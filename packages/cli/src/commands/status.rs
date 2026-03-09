use crate::error::CliError;
use crate::output::table::format_sessions;
use conductor_core::utils::DataPaths;
use std::path::Path;

pub async fn run(
    project: Option<&str>,
    config_path: Option<&Path>,
    json: bool,
) -> Result<(), CliError> {
    let (config, config_file) = if let Some(p) = config_path {
        let cfg = conductor_core::config::load_from_path(p)?;
        (cfg, p.to_path_buf())
    } else {
        conductor_core::config::load_with_path()?
    };

    let mut all_sessions = Vec::new();

    for project_id in config.projects.keys() {
        if let Some(p) = project {
            if project_id != p {
                continue;
            }
        }
        let paths = DataPaths::new(&config_file, project_id);
        let store = conductor_core::session_store::SessionStore::new(paths);
        let sessions = store
            .list()
            .await
            .map_err(|e| CliError::General(e.to_string()))?;
        all_sessions.extend(sessions);
    }

    if json {
        let payload: Vec<_> = all_sessions
            .iter()
            .map(|s| {
                serde_json::json!({
                    "id": s.id,
                    "status": s.status.to_string(),
                    "issue_url": s.issue_url,
                    "branch": s.branch,
                    "created_at": s.created_at,
                    "input_tokens": s.input_tokens,
                    "output_tokens": s.output_tokens,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string(&payload)
                .map_err(|e| CliError::General(format!("JSON serialization failed: {e}")))?
        );
    } else {
        println!("{}", format_sessions(&all_sessions));
    }

    Ok(())
}
