use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::config::ConfigHandle;
use crate::db;
use crate::events::{EventBus, SseEvent};
use crate::health;
use crate::power;
use crate::types::*;

pub struct AppState {
    pub pool: SqlitePool,
    pub config: ConfigHandle,
    pub event_bus: EventBus,
    tasks: RwLock<HashMap<String, (JoinHandle<()>, CancellationToken)>>,
    triggers: RwLock<HashMap<String, Arc<Notify>>>,
    power_tasks: RwLock<HashMap<String, (JoinHandle<()>, CancellationToken)>>,
}

impl AppState {
    pub fn new(pool: SqlitePool, config: ConfigHandle, event_bus: EventBus) -> Arc<Self> {
        Arc::new(Self {
            pool,
            config,
            event_bus,
            tasks: RwLock::new(HashMap::new()),
            triggers: RwLock::new(HashMap::new()),
            power_tasks: RwLock::new(HashMap::new()),
        })
    }

    pub async fn start_health_checks(self: &Arc<Self>) {
        let config = self.config.read().await;
        for server in &config.servers {
            self.start_server_health_task(server).await;
        }
    }

    async fn start_server_health_task(self: &Arc<Self>, server: &ServerConfig) {
        let server_id = server.id.clone();
        let normal_interval = Duration::from_secs(server.check_interval_secs);
        let fast_interval = Duration::from_secs(3);
        let state = Arc::clone(self);
        let token = CancellationToken::new();
        let token_clone = token.clone();
        let trigger = Arc::new(Notify::new());
        let trigger_clone = Arc::clone(&trigger);

        let server_clone = server.clone();
        let server_id_spawn = server_id.clone();
        let handle = tokio::spawn(async move {
            loop {
                // Use a shorter interval while a power transition is in progress.
                let sleep_dur = match db::get_server_state(&state.pool, &server_id_spawn).await {
                    Ok(Some(row)) if matches!(row.power_state, PowerState::PendingOn | PowerState::PendingOff) => fast_interval,
                    _ => normal_interval,
                };

                tokio::select! {
                    _ = token_clone.cancelled() => break,
                    _ = tokio::time::sleep(sleep_dur) => {}
                    _ = trigger_clone.notified() => {}
                }

                state.run_health_check(&server_clone).await;
            }
        });

        self.triggers.write().await.insert(server_id.clone(), trigger);

        let mut tasks = self.tasks.write().await;
        if let Some((old_handle, old_token)) = tasks.remove(&server_id) {
            old_token.cancel();
            old_handle.abort();
        }
        tasks.insert(server_id, (handle, token));
    }

    async fn trigger_fast_check(&self, server_id: &str) {
        if let Some(trigger) = self.triggers.read().await.get(server_id) {
            trigger.notify_one();
        }
    }

    pub async fn run_health_check(self: &Arc<Self>, server: &ServerConfig) {
        let checks = health::run_all_checks(server).await;
        let status = health::compute_status(&checks);
        let now = Utc::now();

        if let Err(e) = db::update_health_status(&self.pool, &server.id, status, &checks, now).await {
            error!("Failed to update health for {}: {e}", server.id);
            return;
        }

        // Check state transitions
        if let Ok(Some(row)) = db::get_server_state(&self.pool, &server.id).await {
            let new_power_state = match (row.power_state, status) {
                (PowerState::PendingOn, ServerStatus::Up) => Some(PowerState::On),
                (PowerState::PendingOff, ServerStatus::Down) => Some(PowerState::Off),
                (PowerState::Failed, ServerStatus::Up) => Some(PowerState::On),
                _ => None,
            };

            if let Some(new_state) = new_power_state {
                if let Err(e) = db::update_power_state(&self.pool, &server.id, new_state).await {
                    error!("Failed to update power state for {}: {e}", server.id);
                }
            }
        }

        // Broadcast update
        let server_state = self.get_server_state(&server.id).await;
        if let Some(state) = server_state {
            self.event_bus.send(SseEvent::Update(state));
        }
    }

    pub async fn run_startup_reconciliation(self: &Arc<Self>) {
        let config = self.config.read().await;
        for server in &config.servers {
            db::ensure_server_exists(&self.pool, &server.id).await.ok();

            // Set config errors
            let error = config.cycle_errors.get(&server.id).map(|s| s.as_str());
            db::update_config_error(&self.pool, &server.id, error).await.ok();

            // Run initial health check
            self.run_health_check(server).await;
        }

        // Reconcile power states
        for server in &config.servers {
            if let Ok(Some(row)) = db::get_server_state(&self.pool, &server.id).await {
                let reconciled = match (row.power_state, row.status) {
                    (PowerState::On | PowerState::PendingOn, ServerStatus::Down) => {
                        Some(PowerState::Off)
                    }
                    (PowerState::Off | PowerState::PendingOff, ServerStatus::Up) => {
                        Some(PowerState::On)
                    }
                    _ => None,
                };

                if let Some(new_state) = reconciled {
                    info!(
                        "Reconciling {}: {:?} -> {:?} (health says {:?})",
                        server.id, row.power_state, new_state, row.status
                    );
                    db::update_power_state(&self.pool, &server.id, new_state).await.ok();
                }
            }
        }
    }

    pub async fn handle_power_on(
        self: &Arc<Self>,
        server_id: &str,
        caller: &str,
    ) -> Result<ServerState, String> {
        let config = self.config.read().await;

        if let Some(err) = config.cycle_errors.get(server_id) {
            return Err(err.clone());
        }

        let server = config
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Server not found: {server_id}"))?
            .clone();

        let deps = server.depends_on.clone();
        drop(config);

        // Every increment propagates to dependencies with a unique caller
        let dep_caller = format!("dep:{server_id}:{caller}");
        for dep_id in &deps {
            self.increment_and_start(dep_id, &dep_caller).await.ok();
        }

        self.increment_and_start(server_id, caller).await?;

        self.get_server_state(server_id)
            .await
            .ok_or_else(|| "State not found".to_string())
    }

    async fn increment_and_start(
        self: &Arc<Self>,
        server_id: &str,
        caller: &str,
    ) -> Result<(), String> {
        let config = self.config.read().await;
        let server = config
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Server not found: {server_id}"))?
            .clone();
        drop(config);

        let row = db::get_server_state(&self.pool, server_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Server state not found: {server_id}"))?;

        if row.callers.contains(&caller.to_string()) {
            return Ok(());
        }

        let new_counter = row.counter + 1;
        let mut new_callers = row.callers.clone();
        new_callers.push(caller.to_string());

        db::update_counter_and_callers(&self.pool, server_id, new_counter, &new_callers)
            .await
            .map_err(|e| e.to_string())?;

        if row.counter == 0 {
            db::update_power_state(&self.pool, server_id, PowerState::PendingOn)
                .await
                .map_err(|e| e.to_string())?;
            self.trigger_fast_check(server_id).await;

            let state = Arc::clone(self);
            let server_clone = server;
            let sid = server_id.to_string();
            let token = CancellationToken::new();
            let token_clone = token.clone();

            let handle = tokio::spawn(async move {
                state.power_on_sequence(&server_clone, token_clone).await;
            });

            let mut power_tasks = self.power_tasks.write().await;
            if let Some((old_handle, old_token)) = power_tasks.remove(server_id) {
                old_token.cancel();
                old_handle.abort();
            }
            power_tasks.insert(sid, (handle, token));
        }

        if let Some(state) = self.get_server_state(server_id).await {
            self.event_bus.send(SseEvent::Update(state));
        }

        Ok(())
    }

    async fn power_on_sequence(self: &Arc<Self>, server: &ServerConfig, cancel: CancellationToken) {
        let timeout = Duration::from_secs(server.power_on_timeout_secs);
        let start = tokio::time::Instant::now();

        // Wait for dependencies to be up
        for dep_id in &server.depends_on {
            loop {
                if cancel.is_cancelled() {
                    return;
                }
                if start.elapsed() > timeout {
                    self.transition_to_failed(&server.id).await;
                    return;
                }
                if let Ok(Some(dep_row)) = db::get_server_state(&self.pool, dep_id).await {
                    if dep_row.status == ServerStatus::Up {
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }

        // Send power on command
        if let Err(e) = power::power_on(server, &self.pool, &server.id).await {
            error!("Power on failed for {}: {e}", server.id);
        }

        // Wait for server to come up or timeout
        loop {
            if cancel.is_cancelled() {
                return;
            }
            if start.elapsed() > timeout {
                self.transition_to_failed(&server.id).await;
                return;
            }
            if let Ok(Some(row)) = db::get_server_state(&self.pool, &server.id).await {
                if row.power_state == PowerState::On {
                    return;
                }
                // Counter was decremented or force-off issued — stop tracking
                if row.power_state != PowerState::PendingOn {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn transition_to_failed(&self, server_id: &str) {
        warn!("Power on timeout for {server_id}, transitioning to failed");
        db::update_power_state(&self.pool, server_id, PowerState::Failed)
            .await
            .ok();
        if let Some(state) = self.get_server_state(server_id).await {
            self.event_bus.send(SseEvent::Update(state));
        }
    }

    pub async fn handle_power_off(
        self: &Arc<Self>,
        server_id: &str,
        caller: &str,
    ) -> Result<ServerState, String> {
        let config = self.config.read().await;
        let server = config
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Server not found: {server_id}"))?
            .clone();

        if let Some(err) = config.cycle_errors.get(server_id) {
            return Err(err.clone());
        }
        drop(config);

        self.decrement_and_stop(server_id, caller, &server).await?;

        self.get_server_state(server_id)
            .await
            .ok_or_else(|| "State not found".to_string())
    }

    async fn decrement_and_stop(
        self: &Arc<Self>,
        server_id: &str,
        caller: &str,
        server: &ServerConfig,
    ) -> Result<(), String> {
        let row = db::get_server_state(&self.pool, server_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Server state not found: {server_id}"))?;

        if !row.callers.contains(&caller.to_string()) {
            return Ok(());
        }

        let mut new_callers = row.callers.clone();
        new_callers.retain(|c| c != caller);
        let new_counter = (row.counter - 1).max(0);

        db::update_counter_and_callers(&self.pool, server_id, new_counter, &new_callers)
            .await
            .map_err(|e| e.to_string())?;

        // Every decrement propagates to dependencies
        let dep_caller = format!("dep:{server_id}:{caller}");
        for dep_id in &server.depends_on {
            self.decrement_dep(dep_id, &dep_caller).await;
        }

        if row.counter == 1 {
            // Cancel in-flight power-on sequence before issuing shutdown
            {
                let mut power_tasks = self.power_tasks.write().await;
                if let Some((handle, token)) = power_tasks.remove(server_id) {
                    token.cancel();
                    handle.abort();
                }
            }

            db::update_power_state(&self.pool, server_id, PowerState::PendingOff)
                .await
                .map_err(|e| e.to_string())?;
            self.trigger_fast_check(server_id).await;

            // Send shutdown command in background
            let server_clone = server.clone();
            let sid = server_id.to_string();
            let pool_clone = self.pool.clone();

            tokio::spawn(async move {
                if let Err(e) = power::power_off(&server_clone, &pool_clone, &sid).await {
                    error!("Power off failed for {}: {e}", sid);
                }
            });
        }

        if let Some(state) = self.get_server_state(server_id).await {
            self.event_bus.send(SseEvent::Update(state));
        }

        Ok(())
    }

    async fn decrement_dep(&self, dep_id: &str, caller: &str) {
        let row = match db::get_server_state(&self.pool, dep_id).await {
            Ok(Some(r)) => r,
            _ => return,
        };

        if !row.callers.contains(&caller.to_string()) {
            return;
        }

        let mut new_callers = row.callers.clone();
        new_callers.retain(|c| c != caller);
        let new_counter = (row.counter - 1).max(0);

        db::update_counter_and_callers(&self.pool, dep_id, new_counter, &new_callers)
            .await
            .ok();

        if row.counter == 1 {
            db::update_power_state(&self.pool, dep_id, PowerState::PendingOff)
                .await
                .ok();
            self.trigger_fast_check(dep_id).await;

            let config = self.config.read().await;
            if let Some(server) = config.servers.iter().find(|s| s.id == dep_id) {
                let server_clone = server.clone();
                let dep_id_owned = dep_id.to_string();
                let pool_clone = self.pool.clone();
                tokio::spawn(async move {
                    if let Err(e) = power::power_off(&server_clone, &pool_clone, &dep_id_owned).await {
                        error!("Power off failed for dep {}: {e}", dep_id_owned);
                    }
                });
            }
        }

        if let Some(state) = self.get_server_state(dep_id).await {
            self.event_bus.send(SseEvent::Update(state));
        }
    }

    pub async fn handle_force_power_on(self: &Arc<Self>, server_id: &str) -> Result<ServerState, String> {
        let config = self.config.read().await;
        let server = config
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Server not found: {server_id}"))?
            .clone();
        drop(config);

        power::power_on(&server, &self.pool, server_id).await?;

        self.get_server_state(server_id)
            .await
            .ok_or_else(|| "State not found".to_string())
    }

    pub async fn handle_force_power_off(self: &Arc<Self>, server_id: &str) -> Result<ServerState, String> {
        let config = self.config.read().await;
        let server = config
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Server not found: {server_id}"))?
            .clone();
        drop(config);

        // Cancel any in-flight power sequence
        {
            let mut power_tasks = self.power_tasks.write().await;
            if let Some((handle, token)) = power_tasks.remove(server_id) {
                token.cancel();
                handle.abort();
            }
        }

        // Send power off command (best effort — don't abort if it fails)
        if let Err(e) = power::power_off(&server, &self.pool, server_id).await {
            error!("Force power off command failed for {}: {e}", server_id);
        }

        // Reset counter, callers and state immediately
        db::update_counter_and_callers(&self.pool, server_id, 0, &[])
            .await
            .map_err(|e| e.to_string())?;
        db::update_power_state(&self.pool, server_id, PowerState::Off)
            .await
            .map_err(|e| e.to_string())?;

        let state = self.get_server_state(server_id).await.ok_or_else(|| "State not found".to_string())?;
        self.event_bus.send(SseEvent::Update(state.clone()));
        Ok(state)
    }

    pub async fn handle_set_counter(
        self: &Arc<Self>,
        server_id: &str,
        value: i32,
    ) -> Result<ServerState, String> {
        db::set_counter(&self.pool, server_id, value)
            .await
            .map_err(|e| e.to_string())?;

        self.get_server_state(server_id)
            .await
            .ok_or_else(|| "State not found".to_string())
    }

    pub async fn get_server_state(&self, server_id: &str) -> Option<ServerState> {
        let config = self.config.read().await;
        let server_config = config.servers.iter().find(|s| s.id == server_id)?;
        let row = db::get_server_state(&self.pool, server_id).await.ok()??;

        Some(ServerState {
            id: row.id,
            name: server_config.name.clone(),
            hostname: server_config.hostname.clone(),
            power_state: row.power_state,
            counter: row.counter,
            callers: row.callers,
            status: row.status,
            checks: row.checks,
            last_checked: row.last_checked,
            config_error: row.config_error,
            depends_on: server_config.depends_on.clone(),
        })
    }

    pub async fn get_all_server_states(&self) -> Vec<ServerState> {
        let config = self.config.read().await;
        let rows = db::get_all_server_states(&self.pool).await.unwrap_or_default();

        rows.into_iter()
            .filter_map(|row| {
                let server_config = config.servers.iter().find(|s| s.id == row.id)?;
                Some(ServerState {
                    id: row.id,
                    name: server_config.name.clone(),
                    hostname: server_config.hostname.clone(),
                    power_state: row.power_state,
                    counter: row.counter,
                    callers: row.callers,
                    status: row.status,
                    checks: row.checks,
                    last_checked: row.last_checked,
                    config_error: row.config_error,
                    depends_on: server_config.depends_on.clone(),
                })
            })
            .collect()
    }

    pub async fn handle_config_reload(self: &Arc<Self>) {
        let config = self.config.read().await;

        // Cancel power tasks for servers whose deps changed
        let mut power_tasks = self.power_tasks.write().await;
        for (id, (handle, token)) in power_tasks.drain() {
            token.cancel();
            handle.abort();
            info!("Cancelled in-flight power task for {id} due to config reload");
            self.event_bus.send(SseEvent::ConfigReloaded {
                server_id: id,
                message: "Power sequence cancelled due to config change".to_string(),
            });
        }
        drop(power_tasks);

        // Restart health check tasks
        let mut tasks = self.tasks.write().await;
        for (_, (handle, token)) in tasks.drain() {
            token.cancel();
            handle.abort();
        }
        drop(tasks);
        self.triggers.write().await.clear();
        drop(config);

        // Re-initialize
        self.run_startup_reconciliation().await;
        self.start_health_checks().await;
    }
}
