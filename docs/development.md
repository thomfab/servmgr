# Development Setup

## Prerequisites

- Rust 1.80+ (install via [rustup](https://rustup.rs))
- Node.js 20+ (for frontend)
- SQLite 3 (usually pre-installed on Linux/macOS)
- `ipmitool` (optional, for IPMI health checks during development)

## Getting Started

### 1. Clone and build the backend

```bash
cd servmgr
cargo build
```

### 2. Install and build the frontend

```bash
cd frontend
npm install
npm run build
```

### 3. Run locally

Create a config directory and config file:

```bash
mkdir -p ./config
cat > ./config/config.yaml << 'EOF'
servers: []
EOF
```

Run the server:

```bash
CONFIG_DIR=./config STATIC_DIR=./frontend/build cargo run
```

The app is now available at http://localhost:8080.

## Development Workflow

### Backend only

```bash
CONFIG_DIR=./config STATIC_DIR=./frontend/build cargo run
```

Changes require restarting the binary. Consider using `cargo-watch`:

```bash
cargo install cargo-watch
CONFIG_DIR=./config STATIC_DIR=./frontend/build cargo watch -x run
```

### Frontend only (with proxy to backend)

```bash
cd frontend
npm run dev
```

The Vite dev server (port 5173) proxies `/api/*` to `localhost:8080`, so the backend must be running.

### Full stack with hot reload

Terminal 1 (backend):
```bash
CONFIG_DIR=./config STATIC_DIR=./frontend/build cargo watch -x run
```

Terminal 2 (frontend dev server):
```bash
cd frontend && npm run dev
```

Use `http://localhost:5173` during development for HMR.

## Running Tests

```bash
cargo test
```

Tests cover:
- Cycle detection (config validation)
- MAC address parsing (WoL)
- Config parsing and defaults

## Building the Docker Image

```bash
docker build -t servmgr .
```

Run:
```bash
docker run --network host --cap-add NET_RAW -v ./config:/config servmgr
```

## Project Structure

```
servmgr/
├── src/
│   ├── main.rs          # Entry point, wiring
│   ├── api.rs           # Axum routes and handlers
│   ├── config.rs        # YAML config loading, validation, hot-reload
│   ├── db.rs            # SQLite schema and queries
│   ├── engine.rs        # Orchestration (health loops, power sequences, state)
│   ├── events.rs        # SSE event bus
│   ├── health.rs        # Health check implementations
│   ├── power.rs         # Power on/off implementations (WoL, IPMI, SSH)
│   └── types.rs         # Shared types and enums
├── frontend/
│   ├── src/
│   │   ├── lib/
│   │   │   ├── api.ts           # API client
│   │   │   ├── sse.ts           # SSE connection helper
│   │   │   ├── types.ts         # TypeScript types
│   │   │   └── components/
│   │   │       └── ServerCard.svelte
│   │   └── routes/
│   │       ├── +layout.svelte   # App shell (nav)
│   │       ├── +layout.ts       # SPA config
│   │       ├── +page.svelte     # Dashboard
│   │       ├── config/+page.svelte    # Config editor
│   │       └── servers/[id]/+page.svelte  # Server detail
│   ├── package.json
│   └── vite.config.ts
├── docs/                # Documentation
├── spec/                # Design spec
├── Cargo.toml
├── Dockerfile
└── .dockerignore
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `8080` | HTTP server port |
| `CONFIG_DIR` | `/config` | Directory for config.yaml and servmgr.db |
| `STATIC_DIR` | `./static` | Directory for frontend static files |
| `RUST_LOG` | `servmgr=info` | Logging level |
