use crate::error::CliError;
use conductor_core::orchestrator::Orchestrator;
use std::path::{Path, PathBuf};

pub async fn run(
    _project: Option<&str>,
    config_path: Option<&Path>,
    json: bool,
) -> Result<(), CliError> {
    let (config, resolved_path) = load_config_with_path(config_path)?;

    if json {
        eprintln!("{}", serde_json::json!({"status": "starting"}));
    } else {
        println!("Starting orchestrator. Press Ctrl-C to stop.");
    }

    let orchestrator = Orchestrator::new(config, &resolved_path)
        .await
        .map_err(|e| CliError::General(e.to_string()))?;

    orchestrator
        .run()
        .await
        .map_err(|e| CliError::General(e.to_string()))?;

    Ok(())
}

fn load_config_with_path(
    path: Option<&Path>,
) -> Result<(conductor_core::config::Config, PathBuf), CliError> {
    if let Some(p) = path {
        let config = conductor_core::config::load_from_path(p).map_err(CliError::Config)?;
        Ok((config, p.to_path_buf()))
    } else {
        conductor_core::config::load_with_path().map_err(CliError::Config)
    }
}
