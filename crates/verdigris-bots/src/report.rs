use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::bot::{BotOutcome, FailureCause};
use crate::stats::DurationStats;

pub const ALL_CAUSES: [FailureCause; 6] = [
    FailureCause::Online,
    FailureCause::Timeout,
    FailureCause::Disconnected,
    FailureCause::ConnectionReset,
    FailureCause::Cancelled,
    FailureCause::Panicked,
];

pub fn cause_name(cause: FailureCause) -> &'static str {
    match cause {
        FailureCause::Online => "online",
        FailureCause::Timeout => "timeout",
        FailureCause::Disconnected => "disconnected",
        FailureCause::ConnectionReset => "connection_reset",
        FailureCause::Cancelled => "cancelled",
        FailureCause::Panicked => "panicked",
    }
}

pub struct Report {
    pub requested: usize,
    pub in_play: usize,
    pub failed: usize,
    pub failures_by_cause: BTreeMap<&'static str, usize>,
    pub disconnect_reasons: BTreeMap<String, usize>,
    pub login_finished: DurationStats,
    pub play: DurationStats,
    pub first_chunk: DurationStats,
    pub total_chunks: u64,
    pub total_bytes: u64,
}

impl Report {
    pub fn build(requested: usize, outcomes: &[BotOutcome]) -> Report {
        let in_play = outcomes.iter().filter(|o| o.is_in_play()).count();
        let failed = outcomes.iter().filter(|o| o.failure.is_some()).count();

        let mut failures_by_cause: BTreeMap<&'static str, usize> = BTreeMap::new();
        for cause in ALL_CAUSES {
            let count = outcomes.iter().filter(|o| o.failure == Some(cause)).count();
            if count > 0 {
                failures_by_cause.insert(cause_name(cause), count);
            }
        }

        let mut disconnect_reasons: BTreeMap<String, usize> = BTreeMap::new();
        for outcome in outcomes {
            if outcome.failure == Some(FailureCause::Disconnected)
                && let Some(reason) = &outcome.disconnect_reason
            {
                *disconnect_reasons.entry(reason.clone()).or_insert(0) += 1;
            }
        }

        let login = collect(outcomes, |o| o.reached_login_finished);
        let play = collect(outcomes, |o| o.reached_play);
        let first = collect(outcomes, |o| o.first_chunk);

        Report {
            requested,
            in_play,
            failed,
            failures_by_cause,
            disconnect_reasons,
            login_finished: DurationStats::from_durations(&login),
            play: DurationStats::from_durations(&play),
            first_chunk: DurationStats::from_durations(&first),
            total_chunks: outcomes.iter().map(|o| o.chunks).sum(),
            total_bytes: outcomes.iter().map(|o| o.bytes).sum(),
        }
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        out.push_str("=== Rapport de charge Verdigris ===\n");
        out.push_str(&format!("bots demandés : {}\n", self.requested));
        out.push_str(&format!("bots en jeu   : {}\n", self.in_play));
        out.push_str(&format!("bots en échec : {}\n", self.failed));
        if self.failures_by_cause.is_empty() {
            out.push_str("  (aucun échec)\n");
        } else {
            for (cause, count) in &self.failures_by_cause {
                out.push_str(&format!("  - {cause} : {count}\n"));
            }
        }
        out.push_str(&format!(
            "login terminé : {}\n",
            format_stats(&self.login_finished)
        ));
        out.push_str(&format!("entrée en jeu : {}\n", format_stats(&self.play)));
        out.push_str(&format!(
            "premier chunk : {}\n",
            format_stats(&self.first_chunk)
        ));
        out.push_str(&format!("chunks reçus  : {}\n", self.total_chunks));
        out.push_str(&format!("octets reçus  : {}\n", self.total_bytes));
        if !self.disconnect_reasons.is_empty() {
            out.push_str("raisons de déconnexion :\n");
            for (reason, count) in &self.disconnect_reasons {
                out.push_str(&format!("  - ({count}x) {reason}\n"));
            }
        }
        out
    }

    pub fn to_json_value(&self) -> JsonReport {
        JsonReport {
            requested: self.requested,
            in_play: self.in_play,
            failed: self.failed,
            failures_by_cause: self.failures_by_cause.clone(),
            disconnect_reasons: self.disconnect_reasons.clone(),
            login_finished: JsonStats::from(&self.login_finished),
            play: JsonStats::from(&self.play),
            first_chunk: JsonStats::from(&self.first_chunk),
            total_chunks: self.total_chunks,
            total_bytes: self.total_bytes,
        }
    }
}

fn collect<F>(outcomes: &[BotOutcome], select: F) -> Vec<Duration>
where
    F: Fn(&BotOutcome) -> Option<Duration>,
{
    outcomes.iter().filter_map(select).collect()
}

fn format_stats(stats: &DurationStats) -> String {
    format!(
        "n={} p50={} p95={} p99={} max={}",
        stats.n,
        format_ms(stats.p50_ms),
        format_ms(stats.p95_ms),
        format_ms(stats.p99_ms),
        format_ms(stats.max_ms),
    )
}

fn format_ms(value: Option<f64>) -> String {
    match value {
        Some(ms) => format!("{ms:.1}ms"),
        None => "—".to_owned(),
    }
}

#[derive(Serialize)]
pub struct JsonReport {
    pub requested: usize,
    pub in_play: usize,
    pub failed: usize,
    pub failures_by_cause: BTreeMap<&'static str, usize>,
    pub disconnect_reasons: BTreeMap<String, usize>,
    pub login_finished: JsonStats,
    pub play: JsonStats,
    pub first_chunk: JsonStats,
    pub total_chunks: u64,
    pub total_bytes: u64,
}

#[derive(Serialize)]
pub struct JsonStats {
    pub n: usize,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub max_ms: Option<f64>,
}

impl From<&DurationStats> for JsonStats {
    fn from(stats: &DurationStats) -> JsonStats {
        JsonStats {
            n: stats.n,
            p50_ms: stats.p50_ms,
            p95_ms: stats.p95_ms,
            p99_ms: stats.p99_ms,
            max_ms: stats.max_ms,
        }
    }
}

pub fn write_json(path: &Path, report: &Report) -> io::Result<()> {
    let json = serde_json::to_string_pretty(&report.to_json_value())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json.as_bytes())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_play_outcome(chunks: u64) -> BotOutcome {
        BotOutcome {
            reached_login_finished: Some(Duration::from_millis(10)),
            reached_play: Some(Duration::from_millis(30)),
            first_chunk: Some(Duration::from_millis(40)),
            chunks,
            bytes: 1000,
            failure: None,
            disconnect_reason: None,
        }
    }

    fn disconnected_outcome(reason: &str) -> BotOutcome {
        BotOutcome {
            reached_login_finished: Some(Duration::from_millis(12)),
            reached_play: None,
            first_chunk: None,
            chunks: 0,
            bytes: 50,
            failure: Some(FailureCause::Disconnected),
            disconnect_reason: Some(reason.to_owned()),
        }
    }

    #[test]
    fn report_counts_in_play_and_failures() {
        let outcomes = vec![
            in_play_outcome(5),
            in_play_outcome(3),
            disconnected_outcome("plein"),
        ];
        let report = Report::build(3, &outcomes);
        assert_eq!(report.requested, 3);
        assert_eq!(report.in_play, 2);
        assert_eq!(report.failed, 1);
        assert_eq!(report.total_chunks, 8);
        assert_eq!(report.failures_by_cause.get("disconnected"), Some(&1));
        assert_eq!(report.disconnect_reasons.get("plein"), Some(&1));
        assert_eq!(report.first_chunk.n, 2);
        assert_eq!(report.play.n, 2);
        assert_eq!(report.login_finished.n, 3);
    }

    #[test]
    fn json_report_serializes_null_for_missing_samples() {
        let outcomes = vec![disconnected_outcome("kick")];
        let report = Report::build(1, &outcomes);
        let json = serde_json::to_string(&report.to_json_value()).unwrap();
        assert!(json.contains("\"first_chunk\":{\"n\":0,\"p50_ms\":null"));
        assert!(json.contains("\"disconnected\":1"));
        assert!(json.contains("\"kick\":1"));
    }
}
