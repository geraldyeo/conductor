use conductor_core::config::Config;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("project '{0}' not found in config")]
    NotFound(String),
    #[error("multiple projects available, specify with -p: {0}")]
    Ambiguous(String),
}

/// Resolve the active project ID from CLI flags + config + CWD.
pub fn resolve_project<'a>(
    config: &'a Config,
    flag: Option<&'a str>,
) -> Result<&'a str, ResolveError> {
    // 1. Explicit flag
    if let Some(id) = flag {
        if config.projects.contains_key(id) {
            return Ok(id);
        }
        return Err(ResolveError::NotFound(id.to_string()));
    }

    // 2. Single project
    if config.projects.len() == 1 {
        return Ok(config.projects.keys().next().unwrap());
    }

    // 3. CWD inside a project's path
    if let Ok(cwd) = std::env::current_dir() {
        for (id, project) in &config.projects {
            let expanded = shellexpand::tilde(&project.path);
            let project_path = Path::new(expanded.as_ref());
            if cwd.starts_with(project_path) {
                return Ok(id);
            }
        }
    }

    // 4. Ambiguous
    let names: Vec<_> = config.projects.keys().cloned().collect();
    Err(ResolveError::Ambiguous(names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_config(projects: Vec<(&str, &str)>) -> Config {
        let mut map = HashMap::new();
        for (id, path) in projects {
            map.insert(
                id.to_string(),
                serde_json::json!({
                    "repo": "owner/repo",
                    "path": path,
                }),
            );
        }
        let value = serde_json::json!({ "projects": map });
        serde_json::from_value(value).expect("invalid test config")
    }

    #[test]
    fn test_explicit_flag_found() {
        let config = make_config(vec![("myapp", "/tmp"), ("other", "/tmp")]);
        let result = resolve_project(&config, Some("myapp")).unwrap();
        assert_eq!(result, "myapp");
    }

    #[test]
    fn test_explicit_flag_not_found() {
        let config = make_config(vec![("myapp", "/tmp")]);
        assert!(matches!(
            resolve_project(&config, Some("other")),
            Err(ResolveError::NotFound(_))
        ));
    }

    #[test]
    fn test_single_project_auto_resolved() {
        let config = make_config(vec![("myapp", "/tmp")]);
        let result = resolve_project(&config, None).unwrap();
        assert_eq!(result, "myapp");
    }

    #[test]
    fn test_multi_project_ambiguous() {
        let config = make_config(vec![("a", "/noexist1"), ("b", "/noexist2")]);
        assert!(matches!(
            resolve_project(&config, None),
            Err(ResolveError::Ambiguous(_))
        ));
    }
}
