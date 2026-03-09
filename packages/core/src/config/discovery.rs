use super::error::ConfigError;
use std::path::PathBuf;

pub fn discover_config_path() -> Result<PathBuf, ConfigError> {
    // 1. Env var override
    if let Ok(val) = std::env::var("AO_CONFIG_PATH") {
        let p = PathBuf::from(val);
        if p.exists() {
            return Ok(p);
        }
        return Err(ConfigError::EnvPathNotFound(p));
    }

    // 2. Walk up from CWD
    let cwd = std::env::current_dir().map_err(|e| ConfigError::Io {
        path: PathBuf::from("."),
        message: e.to_string(),
    })?;
    let mut dir = cwd.as_path();
    loop {
        for name in ["agent-orchestrator.yaml", "agent-orchestrator.yml"] {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    // 3. Home directory fallback
    let home = dirs::home_dir().ok_or(ConfigError::NoHomeDir)?;
    for path in [
        home.join(".agent-orchestrator.yaml"),
        home.join(".agent-orchestrator.yml"),
        home.join(".config/agent-orchestrator/config.yaml"),
    ] {
        if path.exists() {
            return Ok(path);
        }
    }

    Err(ConfigError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_env_override_missing_file() {
        env::set_var("AO_CONFIG_PATH", "/nonexistent/path.yaml");
        let result = discover_config_path();
        env::remove_var("AO_CONFIG_PATH");
        assert!(matches!(result, Err(ConfigError::EnvPathNotFound(_))));
    }
}
