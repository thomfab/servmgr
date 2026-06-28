mod api;
mod config;
mod db;
mod engine;
mod events;
mod health;
mod power;
mod types;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::watch;
use tower_http::services::ServeDir;
use tracing::{error, info};

use crate::config::{create_config_handle, load_config, start_config_watcher, validate_config};
use crate::engine::AppState;
use crate::events::EventBus;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("servmgr=info".parse().unwrap()),
        )
        .init();

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .expect("PORT must be a valid u16");

    let config_dir = PathBuf::from(
        std::env::var("CONFIG_DIR").unwrap_or_else(|_| "/config".to_string()),
    );
    let config_path = config_dir.join("config.yaml");
    let db_path = config_dir.join("servmgr.db");

    // Ensure config directory exists
    std::fs::create_dir_all(&config_dir).expect("Failed to create config directory");

    // Load config
    let app_config = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to load config: {e}");
            std::process::exit(1);
        }
    };
    let validated = validate_config(&app_config);
    let config_handle = create_config_handle(validated);

    // Initialize database
    let pool = db::create_pool(db_path.to_str().unwrap())
        .await
        .expect("Failed to create database pool");

    // Create event bus
    let event_bus = EventBus::new(256);

    // Create app state
    let state = AppState::new(pool, config_handle.clone(), event_bus);

    // Run startup reconciliation
    info!("Running startup reconciliation...");
    state.run_startup_reconciliation().await;

    // Start health check tasks
    state.start_health_checks().await;

    // Start config file watcher
    let (reload_tx, mut reload_rx) = watch::channel(());
    let _watcher = start_config_watcher(config_path.clone(), config_handle.clone(), reload_tx);

    // Config reload background task
    let state_for_reload = Arc::clone(&state);
    let config_path_for_reload = config_path.clone();
    tokio::spawn(async move {
        loop {
            if reload_rx.changed().await.is_err() {
                break;
            }
            info!("Config file changed, reloading...");
            // Small delay to debounce rapid file changes
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Err(e) =
                config::reload_config(&config_path_for_reload, &config_handle).await
            {
                error!("Failed to reload config: {e}");
                continue;
            }
            state_for_reload.handle_config_reload().await;
        }
    });

    // Build router
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "./static".to_string());
    let app = api::router(Arc::clone(&state))
        .fallback_service(ServeDir::new(&static_dir).append_index_html_on_directories(true));

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("servmgr listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind");
    axum::serve(listener, app)
        .await
        .expect("Server error");
}
