use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config;
use crate::db;
use crate::engine::AppState;
use crate::events::SseEvent;
use crate::types::ServerState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/servers", get(list_servers))
        .route("/api/servers/{id}", get(get_server))
        .route("/api/servers/{id}/powerinc", post(power_on))
        .route("/api/servers/{id}/powerdec", post(power_off))
        .route("/api/servers/{id}/poweron", post(force_power_on))
        .route("/api/servers/{id}/poweroff", post(force_power_off))
        .route("/api/servers/{id}/history", get(get_history))
        .route("/api/servers/{id}/powerlog", get(get_power_log))
        .route("/api/servers/{id}/counter", put(set_counter))
        .route("/api/config", get(get_config))
        .route("/api/config", put(put_config))
        .route("/api/events", get(sse_handler))
        .with_state(state)
}

async fn list_servers(State(state): State<Arc<AppState>>) -> Json<Vec<ServerState>> {
    let states = state.get_all_server_states().await;
    Json(states)
}

async fn get_server(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ServerState>, StatusCode> {
    state
        .get_server_state(&id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(Deserialize)]
struct PowerRequest {
    caller: String,
}

async fn power_on(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PowerRequest>,
) -> Result<Json<ServerState>, Response> {
    state
        .handle_power_on(&id, &body.caller)
        .await
        .map(Json)
        .map_err(|e| {
            (StatusCode::CONFLICT, Json(ErrorResponse { error: e })).into_response()
        })
}

async fn power_off(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PowerRequest>,
) -> Result<Json<ServerState>, Response> {
    state
        .handle_power_off(&id, &body.caller)
        .await
        .map(Json)
        .map_err(|e| {
            (StatusCode::CONFLICT, Json(ErrorResponse { error: e })).into_response()
        })
}

async fn force_power_on(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ServerState>, Response> {
    state
        .handle_force_power_on(&id)
        .await
        .map(Json)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })).into_response()
        })
}

async fn force_power_off(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ServerState>, Response> {
    state
        .handle_force_power_off(&id)
        .await
        .map(Json)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e })).into_response()
        })
}

#[derive(Deserialize)]
struct HistoryQuery {
    from: Option<String>,
    to: Option<String>,
}

async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<Vec<db::HistoryEntry>>, StatusCode> {
    let from: Option<DateTime<Utc>> = query.from.and_then(|s| s.parse().ok());
    let to: Option<DateTime<Utc>> = query.to.and_then(|s| s.parse().ok());

    db::get_history(&state.pool, &id, from, to)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
struct CounterRequest {
    value: i32,
}

async fn set_counter(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<CounterRequest>,
) -> Result<Json<ServerState>, StatusCode> {
    state
        .handle_set_counter(&id, body.value)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_power_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<db::PowerLogEntry>>, StatusCode> {
    db::get_power_log(&state.pool, &id, 10)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_config(State(_state): State<Arc<AppState>>) -> Result<String, StatusCode> {
    let config_path = std::path::Path::new("/config/config.yaml");
    std::fs::read_to_string(config_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn put_config(
    State(_state): State<Arc<AppState>>,
    body: String,
) -> Result<StatusCode, Response> {
    let config_path = std::path::Path::new("/config/config.yaml");
    config::save_config(config_path, &body)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })).into_response()
        })
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.event_bus.subscribe();

    // Send full state as first event
    let full_state = state.get_all_server_states().await;
    let initial_event = Event::default()
        .event("full_state")
        .json_data(&full_state)
        .unwrap();

    let stream = async_stream::stream! {
        yield Ok(initial_event);

        loop {
            match rx.recv().await {
                Ok(event) => {
                    let sse_event = match event {
                        SseEvent::FullState(states) => {
                            Event::default()
                                .event("full_state")
                                .json_data(&states)
                                .unwrap()
                        }
                        SseEvent::Update(state) => {
                            Event::default()
                                .event("update")
                                .json_data(&state)
                                .unwrap()
                        }
                        SseEvent::ConfigReloaded { server_id, message } => {
                            Event::default()
                                .event("config_reloaded")
                                .json_data(&serde_json::json!({
                                    "server_id": server_id,
                                    "message": message
                                }))
                                .unwrap()
                        }
                    };
                    yield Ok(sse_event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}
