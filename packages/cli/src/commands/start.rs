use crate::error::CliError;
use conductor_core::orchestrator::Orchestrator;
use std::path::Path;

pub async fn run(
    _project: Option<&str>,
    config_path: Option<&Path>,
    json: bool,
) -> Result<(), CliError> {
    let config = load_config(config_path)?;

    if json {
        eprintln!("{}", serde_json::json!({"status": "starting"}));
    } else {
        println!("Starting orchestrator. Press Ctrl-C to stop.");
    }

    let orchestrator = Orchestrator::new(config)
        .await
        .map_err(|e| CliError::General(e.to_string()))?;

    orchestrator
        .run()
        .await
        .map_err(|e| CliError::General(e.to_string()))?;

    Ok(())
}

fn load_config(path: Option<&Path>) -> Result<conductor_core::config::Config, CliError> {
    if let Some(p) = path {
        conductor_core::config::load_from_path(p).map_err(CliError::Config)
    } else {
        conductor_core::config::load().map_err(CliError::Config)
    }
}
