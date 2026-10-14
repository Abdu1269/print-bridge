use crate::{
    ip_whitelist::{normalize_allowed_ips, REQUIRED_LOOPBACK_IP},
    protocol::EffectivePaper,
};
use serde::{Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

/// Application directory name used in system configuration directories.
pub const APP_CONFIG_DIR_NAME: &str = "com.vergil.printbridge";
/// Agent configuration filename.
pub const CONFIG_FILE_NAME: &str = "config.json";
/// Local task history database filename.
pub const TASK_HISTORY_FILE_NAME: &str = "task_history.sqlite3";
/// Remote task deduplication and status reporting database filename.
pub const REMOTE_STORE_FILE_NAME: &str = "remote.sqlite3";
/// Environment variable name for overriding the configuration file path.
pub const CONFIG_PATH_OVERRIDE_ENV: &str = "PRINT_BRIDGE_CONFIG_PATH";
/// Environment variable name for overriding the data directory path.
pub const DATA_DIR_OVERRIDE_ENV: &str = "PRINT_BRIDGE_DATA_DIR";

/// Minimum user-configurable local service port.
pub const MIN_SERVICE_PORT: u16 = 10000;
/// Maximum user-configurable local service port.
pub const MAX_SERVICE_PORT: u16 = u16::MAX;
/// Default local service port for PrintBridge Agent.
pub const DEFAULT_PORT: u16 = 17890;
/// Minimum allowed remote task polling interval in seconds.
pub const MIN_REMOTE_POLL_INTERVAL_SECONDS: u64 = 3;
/// Minimum allowed remote status report retry count.
pub const MIN_REMOTE_MAX_REPORT_RETRIES: u32 = 1;

/// Return default IP allowlist, preserving local loopback address.
fn default_allowed_ips() -> Vec<String> {
    vec![REQUIRED_LOOPBACK_IP.to_string()]
}

fn default_ui_language() -> UiLanguage {
    UiLanguage::ZhCn
}

/// Return configuration file path used by CLI.
pub fn cli_config_path() -> Result<PathBuf, io::Error> {
    if let Some(path) = env::var_os(CONFIG_PATH_OVERRIDE_ENV) {
        return Ok(PathBuf::from(path));
    }

    Ok(cli_data_dir()?.join(CONFIG_FILE_NAME))
}

/// Return task history database path used by CLI.
pub fn cli_task_history_path() -> Result<PathBuf, io::Error> {
    Ok(cli_data_dir()?.join(TASK_HISTORY_FILE_NAME))
}

/// Return data directory used by CLI and headless serve.
pub fn cli_data_dir() -> Result<PathBuf, io::Error> {
    if let Some(dir) = env::var_os(DATA_DIR_OVERRIDE_ENV) {
        return Ok(PathBuf::from(dir));
    }

    platform_config_dir().map(|dir| dir.join(APP_CONFIG_DIR_NAME))
}

/// Return remote task database path used by headless serve.
pub fn cli_remote_store_path() -> Result<PathBuf, io::Error> {
    Ok(cli_data_dir()?.join(REMOTE_STORE_FILE_NAME))
}

#[cfg(target_os = "windows")]
fn platform_config_dir() -> Result<PathBuf, io::Error> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "APPDATA is not set"))
}

#[cfg(target_os = "macos")]
fn platform_config_dir() -> Result<PathBuf, io::Error> {
    home_dir().map(|home| home.join("Library").join("Application Support"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_config_dir() -> Result<PathBuf, io::Error> {
    if let Some(dir) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(dir));
    }

    home_dir().map(|home| home.join(".config"))
}

#[cfg(unix)]
fn home_dir() -> Result<PathBuf, io::Error> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
}

/// Persisted configuration for local PrintBridge Agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub service: ServiceConfig,
    pub security: SecurityConfig,
    pub printing: PrintingConfig,
    pub limits: LimitsConfig,
    pub app: AppConfig,
    #[serde(default)]
    pub remote: RemoteConfig,
}

/// Local HTTP/WebSocket service binding settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub host: String,
    pub port: u16,
}

/// Browser Origins allowed to open PrintBridge WebSocket sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub allowed_origins: Vec<String>,
    #[serde(default = "default_allowed_ips")]
    pub allowed_ips: Vec<String>,
}

/// Default print settings used when print tasks do not provide overrides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintingConfig {
    pub default_printer: Option<String>,
    pub default_paper: Option<EffectivePaper>,
    pub default_copies: u16,
}

/// Runtime security limits for downloads, batch tasks, and print copies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitsConfig {
    pub max_file_size_mb: u64,
    pub max_batch_jobs: usize,
    pub max_copies: u16,
    pub download_timeout_seconds: u64,
}

/// Desktop application preference settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub autostart: bool,
    #[serde(default = "default_ui_language")]
    pub language: UiLanguage,
}

/// Desktop UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UiLanguage {
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}

/// Remote task polling and status reporting settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub enabled: bool,
    pub endpoint_url: Option<String>,
    pub bearer_token: Option<String>,
    pub device_id: Option<String>,
    pub device_name: Option<String>,
    pub poll_interval_seconds: u64,
    pub max_report_retries: u32,
    pub history_retention_days: u32,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint_url: None,
            bearer_token: None,
            device_id: None,
            device_name: None,
            poll_interval_seconds: 10,
            max_report_retries: 10,
            history_retention_days: 3,
        }
    }
}

impl Default for AgentConfig {
    /// Create first-run configuration using local-only service defaults.
    fn default() -> Self {
        Self {
            service: ServiceConfig {
                host: "127.0.0.1".to_string(),
                port: DEFAULT_PORT,
            },
            security: SecurityConfig {
                allowed_origins: Vec::new(),
                allowed_ips: default_allowed_ips(),
            },
            printing: PrintingConfig {
                default_printer: None,
                default_paper: None,
                default_copies: 1,
            },
            limits: LimitsConfig {
                max_file_size_mb: 20,
                max_batch_jobs: 20,
                max_copies: 100,
                download_timeout_seconds: 30,
            },
            app: AppConfig {
                autostart: false,
                language: UiLanguage::ZhCn,
            },
            remote: RemoteConfig::default(),
        }
    }
}

impl AgentConfig {
    /// Keep compatibility field at localhost default; actual server listen address is determined by server module.
    pub fn normalized(mut self) -> Self {
        self.service.host = "127.0.0.1".to_string();
        self.security.allowed_ips = normalize_allowed_ips(self.security.allowed_ips);
        self
    }

    /// Load configuration from disk; returns default configuration if file does not exist.
    pub fn load(path: &Path) -> Result<Self, io::Error> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)?;
        serde_json::from_str::<Self>(&content)
            .map(Self::normalized)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Save normalized configuration to disk, creating parent directories when necessary.
    pub fn save(&self, path: &Path) -> Result<(), io::Error> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let config = self.clone().normalized();
        let content =
            serde_json::to_string_pretty(&config).expect("AgentConfig should always serialize");
        fs::write(path, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        ffi::OsString,
        sync::{Mutex, OnceLock},
    };
    use uuid::Uuid;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe {
                    env::set_var(self.key, value);
                },
                None => unsafe {
                    env::remove_var(self.key);
                },
            }
        }
    }

    fn set_env(key: &'static str, value: &std::path::Path) -> EnvGuard {
        let previous = env::var_os(key);
        unsafe {
            env::set_var(key, value);
        }
        EnvGuard { key, previous }
    }

    fn clear_env(key: &'static str) -> EnvGuard {
        let previous = env::var_os(key);
        unsafe {
            env::remove_var(key);
        }
        EnvGuard { key, previous }
    }

    #[test]
    fn cli_data_dir_uses_data_dir_override() {
        let _lock = test_lock();
        let data_dir = std::env::temp_dir().join(format!("print-bridge-data-{}", Uuid::new_v4()));
        let _data_guard = set_env(DATA_DIR_OVERRIDE_ENV, &data_dir);
        let _config_guard = clear_env(CONFIG_PATH_OVERRIDE_ENV);

        assert_eq!(cli_data_dir().unwrap(), data_dir);
    }

    #[test]
    fn cli_config_path_prefers_config_path_override() {
        let _lock = test_lock();
        let data_dir = std::env::temp_dir().join(format!("print-bridge-data-{}", Uuid::new_v4()));
        let config_path =
            std::env::temp_dir().join(format!("print-bridge-{}.json", Uuid::new_v4()));
        let _data_guard = set_env(DATA_DIR_OVERRIDE_ENV, &data_dir);
        let _config_guard = set_env(CONFIG_PATH_OVERRIDE_ENV, &config_path);

        assert_eq!(cli_config_path().unwrap(), config_path);
        assert_eq!(
            cli_task_history_path().unwrap(),
            data_dir.join(TASK_HISTORY_FILE_NAME)
        );
        assert_eq!(
            cli_remote_store_path().unwrap(),
            data_dir.join(REMOTE_STORE_FILE_NAME)
        );
    }
}
