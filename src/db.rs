use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use sqlx::Row;

use crate::types::{CheckResult, HealthStatus, PowerState, ServerStatus, display_status_from_str};

pub async fn create_pool(db_path: &str) -> Result<SqlitePool, sqlx::Error> {
    let url = format!("sqlite:{db_path}?mode=rwc");
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS server_state (
            id TEXT PRIMARY KEY,
            power_state TEXT NOT NULL DEFAULT 'off',
            counter INTEGER NOT NULL DEFAULT 0,
            callers TEXT NOT NULL DEFAULT '[]',
            status TEXT NOT NULL DEFAULT 'down',
            checks TEXT NOT NULL DEFAULT '[]',
            last_checked TEXT,
            config_error TEXT
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS status_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_id TEXT NOT NULL,
            status TEXT NOT NULL,
            checks TEXT NOT NULL DEFAULT '[]',
            counter INTEGER DEFAULT 0,
            timestamp TEXT NOT NULL,
            FOREIGN KEY (server_id) REFERENCES server_state(id)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_history_server_time ON status_history(server_id, timestamp)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS power_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            server_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            command TEXT NOT NULL,
            success INTEGER NOT NULL DEFAULT 1,
            message TEXT NOT NULL DEFAULT ''
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_power_log_server ON power_log(server_id, timestamp)",
    )
    .execute(pool)
    .await?;

    // Migration: add counter column for existing DBs (SQLite < 3.37: no NOT NULL without default).
    let _ = sqlx::query("ALTER TABLE status_history ADD COLUMN counter INTEGER DEFAULT 0")
        .execute(pool)
        .await;

    // Migration: drop power_state column (SQLite 3.35+ only; silently ignored on older versions).
    let _ = sqlx::query("ALTER TABLE status_history DROP COLUMN power_state")
        .execute(pool)
        .await;

    Ok(())
}

pub async fn ensure_server_exists(pool: &SqlitePool, server_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR IGNORE INTO server_state (id, power_state, counter, callers, status, checks) VALUES (?, 'off', 0, '[]', 'down', '[]')",
    )
    .bind(server_id)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ServerRow {
    pub id: String,
    pub power_state: PowerState,
    pub counter: i32,
    pub callers: Vec<String>,
    pub status: HealthStatus,
    pub checks: Vec<CheckResult>,
    pub last_checked: Option<DateTime<Utc>>,
    pub config_error: Option<String>,
}

pub async fn get_server_state(pool: &SqlitePool, server_id: &str) -> Result<Option<ServerRow>, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM server_state WHERE id = ?")
        .bind(server_id)
        .fetch_optional(pool)
        .await?;

    match row {
        Some(row) => Ok(Some(parse_server_row(&row))),
        None => Ok(None),
    }
}

pub async fn get_all_server_states(pool: &SqlitePool) -> Result<Vec<ServerRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT * FROM server_state")
        .fetch_all(pool)
        .await?;

    Ok(rows.iter().map(parse_server_row).collect())
}

fn parse_server_row(row: &sqlx::sqlite::SqliteRow) -> ServerRow {
    let id: String = row.get("id");
    let power_state_str: String = row.get("power_state");
    let counter: i32 = row.get("counter");
    let callers_json: String = row.get("callers");
    let status_str: String = row.get("status");
    let checks_json: String = row.get("checks");
    let last_checked_str: Option<String> = row.get("last_checked");
    let config_error: Option<String> = row.get("config_error");

    let power_state = PowerState::from_str(&power_state_str).unwrap_or(PowerState::Off);
    let callers: Vec<String> = serde_json::from_str(&callers_json).unwrap_or_default();
    let status = HealthStatus::from_str(&status_str);
    let checks: Vec<CheckResult> = serde_json::from_str(&checks_json).unwrap_or_default();
    let last_checked = last_checked_str.and_then(|s| s.parse::<DateTime<Utc>>().ok());

    ServerRow {
        id,
        power_state,
        counter,
        callers,
        status,
        checks,
        last_checked,
        config_error,
    }
}

pub async fn update_power_state(
    pool: &SqlitePool,
    server_id: &str,
    power_state: PowerState,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE server_state SET power_state = ? WHERE id = ?")
        .bind(power_state.as_str())
        .bind(server_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_counter_and_callers(
    pool: &SqlitePool,
    server_id: &str,
    counter: i32,
    callers: &[String],
) -> Result<(), sqlx::Error> {
    let callers_json = serde_json::to_string(callers).unwrap_or_else(|_| "[]".to_string());
    sqlx::query("UPDATE server_state SET counter = ?, callers = ? WHERE id = ?")
        .bind(counter)
        .bind(&callers_json)
        .bind(server_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_counter(
    pool: &SqlitePool,
    server_id: &str,
    value: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE server_state SET counter = ?, callers = '[]' WHERE id = ?")
        .bind(value)
        .bind(server_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn update_health_status(
    pool: &SqlitePool,
    server_id: &str,
    health: HealthStatus,
    display_status: &str,
    checks: &[CheckResult],
    counter: i32,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    let checks_json = serde_json::to_string(checks).unwrap_or_else(|_| "[]".to_string());
    let timestamp = now.to_rfc3339();

    sqlx::query(
        "UPDATE server_state SET status = ?, checks = ?, last_checked = ? WHERE id = ?",
    )
    .bind(health.as_str())
    .bind(&checks_json)
    .bind(&timestamp)
    .bind(server_id)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO status_history (server_id, status, checks, counter, timestamp) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(server_id)
    .bind(display_status)
    .bind(&checks_json)
    .bind(counter)
    .bind(&timestamp)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn update_config_error(
    pool: &SqlitePool,
    server_id: &str,
    error: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE server_state SET config_error = ? WHERE id = ?")
        .bind(error)
        .bind(server_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerLogEntry {
    pub id: i64,
    pub server_id: String,
    pub timestamp: DateTime<Utc>,
    pub command: String,
    pub success: bool,
    pub message: String,
}

pub async fn insert_power_log(
    pool: &SqlitePool,
    server_id: &str,
    command: &str,
    success: bool,
    message: &str,
) -> Result<(), sqlx::Error> {
    let timestamp = Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO power_log (server_id, timestamp, command, success, message) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(server_id)
    .bind(&timestamp)
    .bind(command)
    .bind(success as i32)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_power_log(
    pool: &SqlitePool,
    server_id: &str,
    limit: i64,
) -> Result<Vec<PowerLogEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, server_id, timestamp, command, success, message FROM power_log WHERE server_id = ? ORDER BY timestamp DESC LIMIT ?",
    )
    .bind(server_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .iter()
        .map(|row| PowerLogEntry {
            id: row.get("id"),
            server_id: row.get("server_id"),
            timestamp: row
                .get::<String, _>("timestamp")
                .parse()
                .unwrap_or_else(|_| Utc::now()),
            command: row.get("command"),
            success: row.get::<i32, _>("success") != 0,
            message: row.get("message"),
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub server_id: String,
    pub status: ServerStatus,
    pub checks: Vec<CheckResult>,
    pub counter: i32,
    pub timestamp: DateTime<Utc>,
}

pub async fn get_history(
    pool: &SqlitePool,
    server_id: &str,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
) -> Result<Vec<HistoryEntry>, sqlx::Error> {
    let from_str = from
        .unwrap_or_else(|| Utc::now() - chrono::Duration::hours(24))
        .to_rfc3339();
    let to_str = to.unwrap_or_else(Utc::now).to_rfc3339();

    let rows = sqlx::query(
        "SELECT server_id, status, checks, COALESCE(counter, 0) AS counter, timestamp FROM status_history WHERE server_id = ? AND timestamp >= ? AND timestamp <= ? ORDER BY timestamp ASC",
    )
    .bind(server_id)
    .bind(&from_str)
    .bind(&to_str)
    .fetch_all(pool)
    .await?;

    let entries = rows
        .iter()
        .map(|row| {
            let checks_json: String = row.get("checks");
            HistoryEntry {
                server_id: row.get("server_id"),
                status: display_status_from_str(&row.get::<String, _>("status")),
                checks: serde_json::from_str(&checks_json).unwrap_or_default(),
                counter: row.get::<i32, _>("counter"),
                timestamp: row
                    .get::<String, _>("timestamp")
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
            }
        })
        .collect();

    Ok(entries)
}
