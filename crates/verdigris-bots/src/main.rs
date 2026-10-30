#![forbid(unsafe_code)]

use std::process::ExitCode;

use clap::Parser;
use verdigris_bots::orchestrator::{Cli, run, validate};
use verdigris_bots::report::write_json;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config = match validate(&cli) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("erreur de configuration : {message}");
            return ExitCode::from(2);
        }
    };

    let json_path = config.json.clone();
    let report = run(config).await;
    print!("{}", report.to_text());

    if let Some(path) = json_path
        && let Err(err) = write_json(&path, &report)
    {
        eprintln!("échec d'écriture du rapport JSON : {err}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
