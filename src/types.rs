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
    pub label: Option<String>,
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
    #[serde(default = "default_power_timeout", alias = "power_on_timeout_secs")]
    pub power_timeout_secs: u64,
}

fn default_check_interval() -> u64 {
    30
}

fn default_power_timeout() -> u64 {
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

/// Internal health metric derived purely from check results. Stored in DB, used by the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    Up,
    Degraded,
    Down,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Degraded => "degraded",
            Self::Down => "down",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "up" => Self::Up,
            "degraded" => Self::Degraded,
            _ => Self::Down,
        }
    }
}

/// Display status exposed by the API. Derived from counter + health + power transition state.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerStatus {
    Off,
    On,
    TurningOn,
    TurningOff,
    Degraded,
}

pub fn compute_display_status(power_state: PowerState, counter: i32, health: HealthStatus) -> ServerStatus {
    let all_on = health == HealthStatus::Up;
    let all_off = health == HealthStatus::Down;

    if counter == 0 && all_off {
        ServerStatus::Off
    } else if counter > 0 && all_on {
        ServerStatus::On
    } else if counter == 0 {
        // some checks still on — server is in the process of stopping or stuck
        if power_state == PowerState::PendingOff {
            ServerStatus::TurningOff
        } else {
            ServerStatus::Degraded
        }
    } else {
        // counter > 0, some checks off — server is starting or degraded
        if power_state == PowerState::PendingOn {
            ServerStatus::TurningOn
        } else {
            ServerStatus::Degraded
        }
    }
}

/// String form of display status, for storing in history table.
pub fn display_status_str(power_state: PowerState, counter: i32, health: HealthStatus) -> &'static str {
    match compute_display_status(power_state, counter, health) {
        ServerStatus::Off => "off",
        ServerStatus::On => "on",
        ServerStatus::TurningOn => "turning_on",
        ServerStatus::TurningOff => "turning_off",
        ServerStatus::Degraded => "degraded",
    }
}

pub fn display_status_from_str(s: &str) -> ServerStatus {
    match s {
        "on" => ServerStatus::On,
        "turning_on" => ServerStatus::TurningOn,
        "turning_off" => ServerStatus::TurningOff,
        "degraded" => ServerStatus::Degraded,
        // legacy history values stored as up/down/degraded
        "up" => ServerStatus::On,
        _ => ServerStatus::Off,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    #[serde(rename = "type")]
    pub check_type: HealthCheckType,
    pub ok: bool,
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerState {
    pub id: String,
    pub name: String,
    pub hostname: String,
    pub counter: i32,
    pub callers: Vec<String>,
    pub status: ServerStatus,
    pub power_timeout: u64,
    pub checks: Vec<CheckResult>,
    pub last_checked: Option<DateTime<Utc>>,
    pub config_error: Option<String>,
    pub depends_on: Vec<String>,
}
