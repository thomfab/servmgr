use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use sqlx::SqlitePool;
use tokio::sync::{broadcast, Notify, RwLock};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::config::ConfigHandle;
use crate::db;
use crate::events::{EventBus, SseEvent};
use crate::health;
use crate::power;
use crate::types::*;


#[derive(Debug, PartialEq)]
enum WaitOutcome {
    Met,
    TimedOut,
    Cancelled,
}

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

        // Check state transitions and record history with display status
        if let Ok(Some(row)) = db::get_server_state(&self.pool, &server.id).await {
            let disp_str = display_status_str(row.power_state, row.counter, status);
            if let Err(e) = db::update_health_status(&self.pool, &server.id, status, disp_str, &checks, row.counter, now).await {
                error!("Failed to update health for {}: {e}", server.id);
                return;
            }

            let new_power_state = match (row.power_state, status) {
                (PowerState::PendingOn, HealthStatus::Up) => Some(PowerState::On),
                (PowerState::PendingOff, HealthStatus::Down) => Some(PowerState::Off),
                (PowerState::Failed, HealthStatus::Up) => Some(PowerState::On),
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
        self.refresh_health_and_config_errors().await;
        self.reconcile_power_states().await;
    }

    /// Ensures every server exists in the DB, refreshes its config-error
    /// flag, and runs one health check. Safe to call anytime — including on
    /// every hot config reload — since it never touches `power_state`.
    async fn refresh_health_and_config_errors(self: &Arc<Self>) {
        let config = self.config.read().await;
        for server in &config.servers {
            db::ensure_server_exists(&self.pool, &server.id).await.ok();

            // Set config errors
            let error = config.cycle_errors.get(&server.id).map(|s| s.as_str());
            db::update_config_error(&self.pool, &server.id, error).await.ok();

            // Run initial health check
            self.run_health_check(server).await;
        }
    }

    /// Forces `power_state` to match currently observed health when they
    /// disagree (e.g. a server recorded "on" that's actually unreachable).
    /// Only safe right after a genuine process (re)start, when nothing is
    /// actively tracking a server's power-on/off sequence yet — a server
    /// legitimately mid-boot is also `Pending*` + not-yet-healthy, and
    /// forcing it off here would cut its `power_timeout_secs` short. Do NOT
    /// call this from a hot config-reload path.
    async fn reconcile_power_states(self: &Arc<Self>) {
        let config = self.config.read().await;
        for server in &config.servers {
            if let Ok(Some(row)) = db::get_server_state(&self.pool, &server.id).await {
                let reconciled = match (row.power_state, row.status) {
                    (PowerState::On | PowerState::PendingOn, HealthStatus::Down) => {
                        Some(PowerState::Off)
                    }
                    (PowerState::Off | PowerState::PendingOff, HealthStatus::Up) => {
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

        if config.servers.iter().find(|s| s.id == server_id).is_none() {
            return Err(format!("Server not found: {server_id}"));
        }
        drop(config);

        self.increment_and_start(server_id, caller).await?;

        // Cascade the same +1 to every server this one transitively depends
        // on, visiting each server at most once (guards against config
        // cycles; diamond dependencies are fine since each is a distinct
        // edge from a distinct visited source).
        self.cascade_dependencies(server_id, caller, true).await;

        self.get_server_state(server_id)
            .await
            .ok_or_else(|| "State not found".to_string())
    }

    /// Walks the transitive `depends_on` graph starting at `server_id` and
    /// applies the same +1/-1 to every dependency reached, breadth-first.
    /// Each server is visited at most once per call so cyclic configs (which
    /// are flagged elsewhere but not stripped from the graph) can't loop
    /// forever.
    async fn cascade_dependencies(self: &Arc<Self>, server_id: &str, caller: &str, increment: bool) {
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        visited.insert(server_id.to_string());

        let mut queue: std::collections::VecDeque<(String, String)> = std::collections::VecDeque::new();
        {
            let config = self.config.read().await;
            if let Some(server) = config.servers.iter().find(|s| s.id == server_id) {
                let dep_caller = format!("dep:{server_id}:{caller}");
                for dep_id in &server.depends_on {
                    queue.push_back((dep_id.clone(), dep_caller.clone()));
                }
            }
        }

        while let Some((dep_id, dep_caller)) = queue.pop_front() {
            if !visited.insert(dep_id.clone()) {
                continue;
            }

            if increment {
                self.increment_and_start(&dep_id, &dep_caller).await.ok();
            } else {
                self.decrement_and_stop(&dep_id, &dep_caller).await.ok();
            }

            let config = self.config.read().await;
            if let Some(server) = config.servers.iter().find(|s| s.id == dep_id) {
                let next_caller = format!("dep:{dep_id}:{dep_caller}");
                for next_dep_id in &server.depends_on {
                    queue.push_back((next_dep_id.clone(), next_caller.clone()));
                }
            }
        }
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

        // No idempotency/ownership gating: every call is a plain +1.
        let new_counter = row.counter + 1;
        let mut new_callers = row.callers.clone();
        new_callers.push(caller.to_string());

        db::update_counter_and_callers(&self.pool, server_id, new_counter, &new_callers)
            .await
            .map_err(|e| e.to_string())?;
        db::insert_power_log(&self.pool, server_id, "counter_inc", caller, true, &format!("{new_counter}")).await.ok();

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

    /// Waits until `predicate` holds for `watch_id`'s DB row, reacting to
    /// `event_bus` updates instead of polling on a fixed interval. The event
    /// only tells us to recheck the DB — it never carries enough information
    /// on its own to decide the predicate, since `SseEvent::Update` carries
    /// the derived display status, not the raw health/power fields callers
    /// test against.
    async fn wait_for(
        self: &Arc<Self>,
        watch_id: &str,
        deadline: tokio::time::Instant,
        cancel: &CancellationToken,
        mut predicate: impl FnMut(&db::ServerRow) -> bool,
    ) -> WaitOutcome {
        // Subscribe before the first check so a transition landing between
        // the check and the subscribe can't be missed.
        let mut rx = self.event_bus.subscribe();

        loop {
            if let Ok(Some(row)) = db::get_server_state(&self.pool, watch_id).await {
                if predicate(&row) {
                    return WaitOutcome::Met;
                }
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return WaitOutcome::TimedOut;
            }

            tokio::select! {
                _ = cancel.cancelled() => return WaitOutcome::Cancelled,
                _ = tokio::time::sleep(remaining) => return WaitOutcome::TimedOut,
                event = rx.recv() => {
                    match event {
                        Ok(SseEvent::Update(state)) if state.id == watch_id => {}
                        Ok(_) => continue,
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return WaitOutcome::TimedOut,
                    }
                }
            }
        }
    }

    async fn power_on_sequence(self: &Arc<Self>, server: &ServerConfig, cancel: CancellationToken) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(server.power_timeout_secs);

        // Wait for dependencies to be up
        for dep_id in &server.depends_on {
            match self.wait_for(dep_id, deadline, &cancel, |row| row.status == HealthStatus::Up).await {
                WaitOutcome::Met => {}
                WaitOutcome::Cancelled => return,
                WaitOutcome::TimedOut => {
                    self.transition_to_failed(&server.id).await;
                    return;
                }
            }
        }

        // Send power on command
        if let Err(e) = power::power_on(server, &self.pool, &server.id).await {
            error!("Power on failed for {}: {e}", server.id);
        }

        // Wait for server to come up or timeout. Any power_state other than
        // PendingOn (e.g. counter decremented, force-off) also ends the wait
        // without a failure — the task simply stops tracking it.
        let outcome = self
            .wait_for(&server.id, deadline, &cancel, |row| {
                row.power_state == PowerState::On || row.power_state != PowerState::PendingOn
            })
            .await;

        if outcome == WaitOutcome::TimedOut {
            self.transition_to_failed(&server.id).await;
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

        if let Some(err) = config.cycle_errors.get(server_id) {
            return Err(err.clone());
        }

        if config.servers.iter().find(|s| s.id == server_id).is_none() {
            return Err(format!("Server not found: {server_id}"));
        }
        drop(config);

        self.decrement_and_stop(server_id, caller).await?;

        // Cascade the same -1 to every server this one transitively depends
        // on. Symmetric with handle_power_on's cascade.
        self.cascade_dependencies(server_id, caller, false).await;

        self.get_server_state(server_id)
            .await
            .ok_or_else(|| "State not found".to_string())
    }

    /// Decrements `server_id`'s counter by 1, floored at 0. No caller
    /// matching is required — any caller can always bring the counter down,
    /// same as any caller can always bring it up. A counter already at 0 is
    /// a no-op.
    async fn decrement_and_stop(
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

        if row.counter == 0 {
            return Ok(());
        }

        let new_counter = row.counter - 1;
        let mut new_callers = row.callers.clone();
        // Keep the callers list in sync with the counter for the audit/log
        // view: drop a matching entry if there is one, otherwise the oldest.
        if let Some(pos) = new_callers.iter().position(|c| c == caller) {
            new_callers.remove(pos);
        } else if !new_callers.is_empty() {
            new_callers.remove(0);
        }

        db::update_counter_and_callers(&self.pool, server_id, new_counter, &new_callers)
            .await
            .map_err(|e| e.to_string())?;
        db::insert_power_log(&self.pool, server_id, "counter_dec", caller, true, &format!("{new_counter}")).await.ok();

        if new_counter == 0 {
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

    pub async fn handle_force_power_on(self: &Arc<Self>, server_id: &str) -> Result<ServerState, String> {
        let config = self.config.read().await;
        let server = config
            .servers
            .iter()
            .find(|s| s.id == server_id)
            .ok_or_else(|| format!("Server not found: {server_id}"))?
            .clone();
        drop(config);

        db::insert_power_log(&self.pool, server_id, "force_on", "", true, "").await.ok();
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

        db::insert_power_log(&self.pool, server_id, "force_off", "", true, "").await.ok();

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
            counter: row.counter,
            callers: row.callers,
            status: compute_display_status(row.power_state, row.counter, row.status),
            power_timeout: server_config.power_timeout_secs,
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
                    counter: row.counter,
                    callers: row.callers,
                    status: compute_display_status(row.power_state, row.counter, row.status),
                    power_timeout: server_config.power_timeout_secs,
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

        // Re-initialize. Deliberately skips reconcile_power_states — a
        // server mid-power-on/off here is legitimately Pending* and not yet
        // healthy; only a real process restart should treat that as stale.
        self.refresh_health_and_config_errors().await;
        self.start_health_checks().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{create_config_handle, validate_config};

    fn make_server(id: &str, deps: Vec<&str>) -> ServerConfig {
        ServerConfig {
            id: id.to_string(),
            name: id.to_string(),
            hostname: format!("{id}.local"),
            power_on: PowerOnMethod::Wol,
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            wol_broadcast: None,
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
            power_timeout_secs: 300,
        }
    }

    // Returns the AppState plus the backing tempfile — the pool opens new
    // connections lazily, so the file must stay on disk for the test's
    // whole lifetime, not just while the first connection is made.
    async fn make_state(servers: Vec<ServerConfig>) -> (Arc<AppState>, tempfile::NamedTempFile) {
        let db_file = tempfile::NamedTempFile::new().unwrap();
        let pool = db::create_pool(db_file.path().to_str().unwrap()).await.unwrap();
        for s in &servers {
            db::ensure_server_exists(&pool, &s.id).await.unwrap();
        }

        let validated = validate_config(&AppConfig { servers });
        let config = create_config_handle(validated);
        let event_bus = EventBus::new(16);
        (AppState::new(pool, config, event_bus), db_file)
    }

    async fn counter_of(state: &Arc<AppState>, id: &str) -> i32 {
        db::get_server_state(&state.pool, id).await.unwrap().unwrap().counter
    }

    /// Reproduces the live bug report: a server mid-boot (PendingOn, still
    /// Down because it hasn't finished booting yet — well within its
    /// power_timeout_secs) must not be force-reconciled to Off just because
    /// a config file save (e.g. editing some other, unrelated server)
    /// triggered a hot-reload. Only a genuine process restart should trust
    /// stale DB state enough to force Pending* -> Off/On.
    #[tokio::test]
    async fn config_reload_does_not_kill_a_server_still_mid_power_on() {
        let (state, _db_file) = make_state(vec![make_server("booting", vec![])]).await;

        db::update_power_state(&state.pool, "booting", PowerState::PendingOn).await.unwrap();

        state.handle_config_reload().await;

        let row = db::get_server_state(&state.pool, "booting").await.unwrap().unwrap();
        assert_eq!(
            row.power_state,
            PowerState::PendingOn,
            "a config reload must not force a still-booting server to Off"
        );
    }

    /// Point 1 of the bug report: -1 always works down to 0, and is a no-op
    /// (not an error) once the counter is already 0 — no caller matching
    /// required.
    #[tokio::test]
    async fn decrement_floors_at_zero_and_needs_no_matching_caller() {
        let (state, _db_file) = make_state(vec![make_server("a", vec![])]).await;

        state.handle_power_on("a", "webui-1").await.unwrap();
        state.handle_power_on("a", "webui-2").await.unwrap();
        assert_eq!(counter_of(&state, "a").await, 2);

        // Decrement with a caller string that was never used to increment —
        // this is exactly the original bug (UI's caller didn't match).
        state.handle_power_off("a", "some-unrelated-caller").await.unwrap();
        assert_eq!(counter_of(&state, "a").await, 1);

        state.handle_power_off("a", "some-unrelated-caller").await.unwrap();
        assert_eq!(counter_of(&state, "a").await, 0);

        // Already at 0: further decrements are harmless no-ops, not errors.
        state.handle_power_off("a", "some-unrelated-caller").await.unwrap();
        assert_eq!(counter_of(&state, "a").await, 0);
    }

    /// Point 2 of the bug report: incrementing/decrementing a server
    /// cascades to every server it depends on, transitively (a -> b -> c),
    /// not just the direct dependency.
    #[tokio::test]
    async fn cascade_reaches_transitive_dependencies() {
        let (state, _db_file) = make_state(vec![
            make_server("a", vec!["b"]),
            make_server("b", vec!["c"]),
            make_server("c", vec![]),
        ]).await;

        state.handle_power_on("a", "webui-1").await.unwrap();
        assert_eq!(counter_of(&state, "a").await, 1);
        assert_eq!(counter_of(&state, "b").await, 1, "direct dependency should be incremented");
        assert_eq!(counter_of(&state, "c").await, 1, "transitive dependency should also be incremented");

        state.handle_power_off("a", "webui-1").await.unwrap();
        assert_eq!(counter_of(&state, "a").await, 0);
        assert_eq!(counter_of(&state, "b").await, 0);
        assert_eq!(counter_of(&state, "c").await, 0);
    }

    /// Reproduces the original report directly: server1 depends on server2;
    /// server2 is only ever incremented via the dependency cascade (never
    /// directly), and calling -1 straight on server2 must still work.
    #[tokio::test]
    async fn direct_decrement_on_cascade_only_dependency_works() {
        let (state, _db_file) = make_state(vec![
            make_server("server1", vec!["server2"]),
            make_server("server2", vec![]),
        ]).await;

        state.handle_power_on("server1", "webui-1").await.unwrap();
        assert_eq!(counter_of(&state, "server2").await, 1);

        // -1 called directly on server2, via the API, same as clicking the
        // button in the UI.
        state.handle_power_off("server2", "webui-9").await.unwrap();
        assert_eq!(counter_of(&state, "server2").await, 0);
    }

    /// A diamond dependency (a -> b, a -> c, b -> d, c -> d) must not loop
    /// forever, and a single +1 on `a` must apply exactly one +1 to `d` —
    /// not one per path — so a single button click never produces a
    /// surprising jump on a shared dependency.
    #[tokio::test]
    async fn diamond_dependency_cascade_does_not_double_count_or_loop() {
        let (state, _db_file) = make_state(vec![
            make_server("a", vec!["b", "c"]),
            make_server("b", vec!["d"]),
            make_server("c", vec!["d"]),
            make_server("d", vec![]),
        ]).await;

        state.handle_power_on("a", "webui-1").await.unwrap();
        assert_eq!(counter_of(&state, "d").await, 1, "d reached via two paths from a single action, but only counted once");

        state.handle_power_off("a", "webui-1").await.unwrap();
        assert_eq!(counter_of(&state, "d").await, 0);

        // Two independent top-level actions that each reach d still each
        // contribute their own count, and unwind independently.
        state.handle_power_on("a", "webui-2").await.unwrap();
        state.handle_power_on("b", "webui-3").await.unwrap();
        assert_eq!(counter_of(&state, "d").await, 2, "two separate actions, each reaching d, both count");

        state.handle_power_off("a", "webui-2").await.unwrap();
        assert_eq!(counter_of(&state, "d").await, 1, "a's action released; b's is still outstanding");

        state.handle_power_off("b", "webui-3").await.unwrap();
        assert_eq!(counter_of(&state, "d").await, 0);
    }

    /// `wait_for` must resolve as soon as a matching event_bus update arrives,
    /// not only on its next poll tick. The deadline here (5s) is far longer
    /// than the 500ms the test allows, so a pass only happens if the event
    /// itself woke the waiter.
    #[tokio::test]
    async fn wait_for_reacts_to_event_instead_of_waiting_for_a_poll_tick() {
        let (state, _db_file) = make_state(vec![make_server("dep", vec![])]).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let wait_state = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            let cancel = CancellationToken::new();
            wait_state
                .wait_for("dep", deadline, &cancel, |row| row.status == HealthStatus::Up)
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        db::update_health_status(&state.pool, "dep", HealthStatus::Up, "on", &[], 0, Utc::now())
            .await
            .unwrap();
        let updated = state.get_server_state("dep").await.unwrap();
        state.event_bus.send(SseEvent::Update(updated));

        let outcome = tokio::time::timeout(Duration::from_millis(500), handle)
            .await
            .expect("wait_for should react to the event well before the 5s deadline")
            .unwrap();

        assert!(matches!(outcome, WaitOutcome::Met));
    }

    /// If the predicate never becomes true, `wait_for` must give up at the
    /// deadline rather than waiting forever.
    #[tokio::test]
    async fn wait_for_times_out_when_predicate_never_holds() {
        let (state, _db_file) = make_state(vec![make_server("dep", vec![])]).await;

        let deadline = tokio::time::Instant::now() + Duration::from_millis(150);
        let cancel = CancellationToken::new();

        let outcome = state
            .wait_for("dep", deadline, &cancel, |row| row.status == HealthStatus::Up)
            .await;

        assert_eq!(outcome, WaitOutcome::TimedOut);
    }

    /// Cancelling the token must interrupt the wait immediately, even with a
    /// deadline that's still far away.
    #[tokio::test]
    async fn wait_for_stops_promptly_when_cancelled() {
        let (state, _db_file) = make_state(vec![make_server("dep", vec![])]).await;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let wait_state = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            wait_state
                .wait_for("dep", deadline, &cancel_clone, |row| row.status == HealthStatus::Up)
                .await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        cancel.cancel();

        let outcome = tokio::time::timeout(Duration::from_millis(500), handle)
            .await
            .expect("cancellation should stop the wait well before the 30s deadline")
            .unwrap();

        assert_eq!(outcome, WaitOutcome::Cancelled);
    }

    /// End-to-end: `power_on_sequence`'s dependency wait must proceed as soon
    /// as the dependency's health-check event fires, not on the old loop's
    /// 5s poll tick. `main`'s power_timeout_secs (300s, via make_server) is
    /// far longer than this test's window, so completing in time only
    /// happens if the event itself unblocked the wait.
    #[tokio::test]
    async fn power_on_sequence_proceeds_as_soon_as_dependency_event_fires() {
        let (state, _db_file) = make_state(vec![
            make_server("main", vec!["dep"]),
            make_server("dep", vec![]),
        ]).await;

        db::update_power_state(&state.pool, "main", PowerState::PendingOn).await.unwrap();

        let main_config = {
            let config = state.config.read().await;
            config.servers.iter().find(|s| s.id == "main").unwrap().clone()
        };

        let seq_state = Arc::clone(&state);
        let handle = tokio::spawn(async move {
            seq_state.power_on_sequence(&main_config, CancellationToken::new()).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        db::update_health_status(&state.pool, "dep", HealthStatus::Up, "on", &[], 0, Utc::now())
            .await
            .unwrap();
        let updated = state.get_server_state("dep").await.unwrap();
        state.event_bus.send(SseEvent::Update(updated));

        tokio::time::sleep(Duration::from_millis(200)).await;
        handle.abort();

        let log = db::get_power_log(&state.pool, "main", 5).await.unwrap();
        assert!(
            log.iter().any(|e| e.command == "wol"),
            "power-on command should have been sent once the dependency came up, well within 250ms"
        );
    }

    /// If the dependency never comes up, the dependent must still fail at
    /// its configured power_timeout_secs rather than waiting forever for an
    /// event that's never coming.
    #[tokio::test]
    async fn power_on_sequence_fails_when_dependency_never_comes_up() {
        let mut main = make_server("main", vec!["dep"]);
        main.power_timeout_secs = 1;
        let (state, _db_file) = make_state(vec![main.clone(), make_server("dep", vec![])]).await;

        db::update_power_state(&state.pool, "main", PowerState::PendingOn).await.unwrap();

        state.power_on_sequence(&main, CancellationToken::new()).await;

        let row = db::get_server_state(&state.pool, "main").await.unwrap().unwrap();
        assert_eq!(row.power_state, PowerState::Failed);
    }
}
