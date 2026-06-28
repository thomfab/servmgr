use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowerOnMethod {
    Wol,
    Ipmi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowerOffMethod {
    Ssh,
    Ipmi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    Ping,
    Http,
    Tcp,
    Ssh,
    IpmiPower,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    #[serde(rename = "type")]
    pub check_type: HealthCheckType,
    pub url: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub power_on: PowerOnMethod,
    pub mac: Option<String>,
    pub wol_broadcast: Option<String>,
    pub power_off: PowerOffMethod,
    pub ssh_user: Option<String>,
    pub ssh_key_path: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_shutdown_cmd: Option<String>,
    pub ipmi_ip: Option<String>,
    pub ipmi_user: Option<String>,
    pub ipmi_password: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub health_checks: Vec<HealthCheckConfig>,
    #[serde(default = "default_check_interval")]
    pub check_interval_secs: u64,
    #[serde(default = "default_power_on_timeout")]
    pub power_on_timeout_secs: u64,
}

fn default_check_interval() -> u64 {
    30
}

fn default_power_on_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub servers: Vec<ServerConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { servers: vec![] }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(rename_all = "snake_case")]
pub enum PowerState {
    Off,
    PendingOn,
    On,
    PendingOff,
    Failed,
}

impl PowerState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::PendingOn => "pending_on",
            Self::On => "on",
            Self::PendingOff => "pending_off",
            Self::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "pending_on" => Some(Self::PendingOn),
            "on" => Some(Self::On),
            "pending_off" => Some(Self::PendingOff),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Up,
    Degraded,
    Down,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    #[serde(rename = "type")]
    pub check_type: HealthCheckType,
    pub ok: bool,
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub power_state: PowerState,
    pub counter: i32,
    pub callers: Vec<String>,
    pub status: ServerStatus,
    pub checks: Vec<CheckResult>,
    pub last_checked: Option<DateTime<Utc>>,
    pub config_error: Option<String>,
    pub depends_on: Vec<String>,
}
