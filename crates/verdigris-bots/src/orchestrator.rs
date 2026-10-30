use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;
use tokio::task::JoinSet;
use tokio::time::{interval, sleep, timeout};
use tokio_util::sync::CancellationToken;

use crate::bot::{BotConfig, BotOutcome, FailureCause, run_bot};
use crate::metrics::Metrics;
use crate::report::Report;

pub const REPORT_INTERVAL: Duration = Duration::from_secs(5);
pub const JOIN_BUDGET: Duration = Duration::from_secs(10);

#[derive(Parser, Debug, Clone)]
#[command(
    name = "verdigris-bots",
    about = "Générateur de charge pour serveurs Minecraft Java 26.3 (protocole 777, mode hors ligne)"
)]
pub struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 25565)]
    pub port: u16,
    #[arg(long)]
    pub count: usize,
    #[arg(long)]
    pub rate: f64,
    #[arg(long, default_value_t = 60)]
    pub duration: u64,
    #[arg(long, default_value = "Bot")]
    pub prefix: String,
    #[arg(long = "view-distance", default_value_t = 8)]
    pub view_distance: u8,
    #[arg(long, default_value_t = 0.0)]
    pub walk: f64,
    #[arg(long)]
    pub json: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub count: usize,
    pub spawn_interval: Duration,
    pub duration: Duration,
    pub prefix: String,
    pub view_distance: u8,
    pub walk: f64,
    pub json: Option<PathBuf>,
}

pub fn validate(cli: &Cli) -> Result<Config, String> {
    if cli.count == 0 {
        return Err("--count doit être strictement positif".to_owned());
    }
    if !cli.rate.is_finite() || cli.rate <= 0.0 {
        return Err("--rate doit être un nombre fini strictement positif".to_owned());
    }
    if !cli.walk.is_finite() || cli.walk < 0.0 {
        return Err("--walk doit être un nombre fini positif ou nul".to_owned());
    }
    Ok(Config {
        host: cli.host.clone(),
        port: cli.port,
        count: cli.count,
        spawn_interval: Duration::from_secs_f64(1.0 / cli.rate),
        duration: Duration::from_secs(cli.duration),
        prefix: cli.prefix.clone(),
        view_distance: cli.view_distance,
        walk: cli.walk,
        json: cli.json.clone(),
    })
}

pub fn bot_name(prefix: &str, index: usize) -> String {
    format!("{prefix}{:04}", index + 1)
}

pub async fn run(config: Config) -> Report {
    let token = CancellationToken::new();
    let metrics = Arc::new(Metrics::new());

    spawn_signal_watch(token.clone());
    let reporter = spawn_reporter(token.clone(), Arc::clone(&metrics));

    let mut join_set: JoinSet<BotOutcome> = JoinSet::new();
    let mut spawned = 0usize;

    for index in 0..config.count {
        if token.is_cancelled() {
            break;
        }
        let bot_cfg = BotConfig {
            host: config.host.clone(),
            port: config.port,
            name: bot_name(&config.prefix, index),
            view_distance: config.view_distance,
            walk: config.walk,
        };
        let bot_token = token.clone();
        let bot_metrics = Arc::clone(&metrics);
        join_set.spawn(async move { run_bot(bot_cfg, index, bot_token, bot_metrics).await });
        spawned += 1;

        if index + 1 < config.count {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = sleep(config.spawn_interval) => {}
            }
        }
    }

    tokio::select! {
        _ = token.cancelled() => {}
        _ = sleep(config.duration) => {}
    }
    token.cancel();
    reporter.abort();

    let outcomes = join_all(&mut join_set, spawned).await;
    Report::build(config.count, &outcomes)
}

fn spawn_signal_watch(token: CancellationToken) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            token.cancel();
        }
    });
}

fn spawn_reporter(token: CancellationToken, metrics: Arc<Metrics>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = interval(REPORT_INTERVAL);
        ticker.tick().await;
        let mut prev = metrics.snapshot();
        loop {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = ticker.tick() => {
                    let now = metrics.snapshot();
                    let secs = REPORT_INTERVAL.as_secs_f64();
                    let chunks_per_s = now.chunks.saturating_sub(prev.chunks) as f64 / secs;
                    let bytes_per_s = now.bytes.saturating_sub(prev.bytes) as f64 / secs;
                    println!(
                        "[+5s] en jeu={} échecs={} chunks/s={:.1} débit={:.1} Kio/s",
                        now.in_play,
                        now.failures,
                        chunks_per_s,
                        bytes_per_s / 1024.0,
                    );
                    prev = now;
                }
            }
        }
    })
}

async fn join_all(join_set: &mut JoinSet<BotOutcome>, spawned: usize) -> Vec<BotOutcome> {
    let mut outcomes = Vec::with_capacity(spawned);
    let deadline = Instant::now() + JOIN_BUDGET;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match timeout(remaining, join_set.join_next()).await {
            Ok(Some(Ok(outcome))) => outcomes.push(outcome),
            Ok(Some(Err(err))) => {
                if err.is_panic() {
                    outcomes.push(BotOutcome::panicked());
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    while outcomes.len() < spawned {
        let mut timed_out = BotOutcome::panicked();
        timed_out.failure = Some(FailureCause::Timeout);
        outcomes.push(timed_out);
    }
    join_set.abort_all();
    outcomes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cli() -> Cli {
        Cli {
            host: "127.0.0.1".to_owned(),
            port: 25565,
            count: 10,
            rate: 5.0,
            duration: 60,
            prefix: "Bot".to_owned(),
            view_distance: 8,
            walk: 0.0,
            json: None,
        }
    }

    #[test]
    fn valid_cli_is_accepted() {
        let config = validate(&base_cli()).unwrap();
        assert_eq!(config.count, 10);
        assert_eq!(config.spawn_interval, Duration::from_secs_f64(0.2));
        assert_eq!(config.duration, Duration::from_secs(60));
    }

    #[test]
    fn count_zero_is_rejected() {
        let mut cli = base_cli();
        cli.count = 0;
        assert!(validate(&cli).is_err());
    }

    #[test]
    fn rate_zero_is_rejected() {
        let mut cli = base_cli();
        cli.rate = 0.0;
        assert!(validate(&cli).is_err());
    }

    #[test]
    fn rate_negative_is_rejected() {
        let mut cli = base_cli();
        cli.rate = -1.0;
        assert!(validate(&cli).is_err());
    }

    #[test]
    fn rate_non_finite_is_rejected() {
        let mut cli = base_cli();
        cli.rate = f64::INFINITY;
        assert!(validate(&cli).is_err());
        cli.rate = f64::NAN;
        assert!(validate(&cli).is_err());
    }

    #[test]
    fn walk_negative_is_rejected() {
        let mut cli = base_cli();
        cli.walk = -0.1;
        assert!(validate(&cli).is_err());
    }

    #[test]
    fn walk_non_finite_is_rejected() {
        let mut cli = base_cli();
        cli.walk = f64::NAN;
        assert!(validate(&cli).is_err());
    }

    #[test]
    fn bot_names_are_padded_and_one_based() {
        assert_eq!(bot_name("Bot", 0), "Bot0001");
        assert_eq!(bot_name("Bot", 199), "Bot0200");
        assert_eq!(bot_name("Charge", 9), "Charge0010");
    }
}
