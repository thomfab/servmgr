# Technical Architecture

## Languages

- **Backend**: Rust (2024 edition)
- **Frontend**: TypeScript + Svelte 5 (SvelteKit with static adapter)

## Frameworks & Libraries

### Backend (Rust)

| Crate | Purpose |
|-------|---------|
| axum 0.8 | HTTP framework (REST API, SSE) |
| tokio | Async runtime |
| sqlx 0.8 (sqlite) | Database access |
| serde + serde_yaml | Config parsing |
| serde_json | JSON serialization |
| reqwest | HTTP health checks |
| surge-ping | ICMP ping |
| notify 7 | Filesystem watcher for config hot-reload |
| tower-http | Static file serving |
| chrono | Timestamps (ISO 8601) |
| tracing | Structured logging |
| tokio-util | Cancellation tokens |

### Frontend (TypeScript)

| Package | Purpose |
|---------|---------|
| SvelteKit | Application framework |
| @sveltejs/adapter-static | Static site generation (SPA mode) |
| Vite | Build tool / dev server |

## Deployment

Single Docker container running the Rust binary with embedded static files.

- **Base image**: `debian:bookworm-slim`
- **Required runtime deps**: `ipmitool`, `openssh-client`, `ca-certificates`
- **Network**: Must use `--network host` (WoL broadcasts + IPMI LAN access)
- **Capabilities**: `CAP_NET_RAW` (ICMP ping)
- **Volume**: `/config` — holds `config.yaml` and `servmgr.db`
- **Port**: Configurable via `PORT` env var (default 8080)

## Architecture Pattern

The backend follows a layered architecture:

1. **API layer** (`api.rs`) — HTTP handlers, request/response types
2. **Engine** (`engine.rs`) — Orchestrates health checks, power management, state transitions
3. **Domain modules** — `health.rs`, `power.rs`, `config.rs` — pure domain logic
4. **Persistence** (`db.rs`) — SQLite via sqlx
5. **Events** (`events.rs`) — Broadcast channel for SSE

All state is held in `AppState` (shared via `Arc`) with the SQLite pool as the source of truth. Background tasks (health checks, power sequences) communicate state changes through the database and the event bus.
