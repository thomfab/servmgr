use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::{watch, RwLock};
use tracing::{error, info};

use crate::types::{AppConfig, ServerConfig};

#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    pub servers: Vec<ServerConfig>,
    pub cycle_errors: HashMap<String, String>,
}

pub fn load_config(path: &Path) -> Result<AppConfig, String> {
    if !path.exists() {
        let default = AppConfig::default();
        let yaml = serde_yaml::to_string(&default).map_err(|e| e.to_string())?;
        std::fs::write(path, &yaml).map_err(|e| format!("Failed to write default config: {e}"))?;
        info!("No config found, created default at {}", path.display());
        return Ok(default);
    }
    let content = std::fs::read_to_string(path).map_err(|e| format!("Failed to read config: {e}"))?;
    let config: AppConfig = serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse config: {e}"))?;
    Ok(config)
}

pub fn validate_config(config: &AppConfig) -> ValidatedConfig {
    let cycle_errors = detect_cycles(&config.servers);
    ValidatedConfig {
        servers: config.servers.clone(),
        cycle_errors,
    }
}

fn detect_cycles(servers: &[ServerConfig]) -> HashMap<String, String> {
    let ids: HashSet<&str> = servers.iter().map(|s| s.id.as_str()).collect();
    let mut errors = HashMap::new();

    for server in servers {
        let mut visited = HashSet::new();
        let mut path = Vec::new();
        if has_cycle(server.id.as_str(), servers, &ids, &mut visited, &mut path) {
            path.push(server.id.clone());
            let cycle_str = path.join(" → ");
            errors.insert(server.id.clone(), format!("Cycle detected: {cycle_str}"));
        }
    }

    errors
}

fn has_cycle(
    start: &str,
    servers: &[ServerConfig],
    valid_ids: &HashSet<&str>,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
) -> bool {
    let server = match servers.iter().find(|s| s.id == start) {
        Some(s) => s,
        None => return false,
    };

    for dep in &server.depends_on {
        if !valid_ids.contains(dep.as_str()) {
            continue;
        }
        if path.first().map(|s| s.as_str()) == Some(dep.as_str()) {
            path.push(dep.clone());
            return true;
        }
        if visited.contains(dep) {
            continue;
        }
        visited.insert(dep.clone());
        path.push(dep.clone());
        if has_cycle(dep, servers, valid_ids, visited, path) {
            return true;
        }
        path.pop();
    }

    false
}

pub type ConfigHandle = Arc<RwLock<ValidatedConfig>>;

pub fn create_config_handle(config: ValidatedConfig) -> ConfigHandle {
    Arc::new(RwLock::new(config))
}

pub fn start_config_watcher(
    config_path: PathBuf,
    _config_handle: ConfigHandle,
    reload_tx: watch::Sender<()>,
) -> Option<RecommendedWatcher> {
    let watch_config = config_path.file_name().unwrap_or_default().to_owned();
    let mut watcher = match RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    let is_config = event.paths.iter().any(|p| {
                        p.file_name().map(|n| n == watch_config).unwrap_or(false)
                    });
                    if is_config {
                        let _ = reload_tx.send(());
                    }
                }
            }
        },
        notify::Config::default(),
    ) {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to create file watcher: {e}");
            return None;
        }
    };

    let watch_path = config_path.parent().unwrap_or(Path::new("/config"));
    if let Err(e) = watcher.watch(watch_path, RecursiveMode::NonRecursive) {
        error!("Failed to watch config directory: {e}");
        return None;
    }

    info!("Watching {} for config changes", config_path.display());
    Some(watcher)
}

pub async fn reload_config(config_path: &Path, config_handle: &ConfigHandle) -> Result<(), String> {
    let config = load_config(config_path)?;
    let validated = validate_config(&config);
    let mut handle = config_handle.write().await;
    *handle = validated;
    info!("Config reloaded successfully");
    Ok(())
}

pub async fn save_config(config_path: &Path, yaml_content: &str) -> Result<(), String> {
    let _config: AppConfig =
        serde_yaml::from_str(yaml_content).map_err(|e| format!("Invalid YAML: {e}"))?;
    std::fs::write(config_path, yaml_content)
        .map_err(|e| format!("Failed to write config: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    fn make_server(id: &str, deps: Vec<&str>) -> ServerConfig {
        ServerConfig {
            id: id.to_string(),
            name: id.to_string(),
            hostname: format!("{id}.local"),
            power_on: PowerOnMethod::Wol,
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            power_off: PowerOffMethod::Ssh,
            ssh_user: Some("user".to_string()),
            ssh_key_path: Some("/key".to_string()),
            ssh_password: None,
            ssh_shutdown_cmd: None,
            ipmi_ip: None,
            ipmi_user: None,
            ipmi_password: None,
            depends_on: deps.into_iter().map(String::from).collect(),
            health_checks: vec![],
            check_interval_secs: 30,
            power_on_timeout_secs: 300,
        }
    }

    #[test]
    fn test_no_cycles() {
        let servers = vec![
            make_server("a", vec!["b"]),
            make_server("b", vec!["c"]),
            make_server("c", vec![]),
        ];
        let errors = detect_cycles(&servers);
        assert!(errors.is_empty());
    }

    #[test]
    fn test_direct_cycle() {
        let servers = vec![
            make_server("a", vec!["b"]),
            make_server("b", vec!["a"]),
        ];
        let errors = detect_cycles(&servers);
        assert!(!errors.is_empty());
        assert!(errors.contains_key("a") || errors.contains_key("b"));
    }

    #[test]
    fn test_three_node_cycle() {
        let servers = vec![
            make_server("a", vec!["b"]),
            make_server("b", vec!["c"]),
            make_server("c", vec!["a"]),
        ];
        let errors = detect_cycles(&servers);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_self_cycle() {
        let servers = vec![make_server("a", vec!["a"])];
        let errors = detect_cycles(&servers);
        assert!(errors.contains_key("a"));
    }

    #[test]
    fn test_nonexistent_dep_ignored() {
        let servers = vec![make_server("a", vec!["nonexistent"])];
        let errors = detect_cycles(&servers);
        assert!(errors.is_empty());
    }
}
