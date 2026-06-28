# servmgr Design Spec

_Date: 2026-06-27_

## Context

servmgr is a home network server manager. It lets you turn servers on and off, monitor their health, and manage power dependencies between them — all from a mobile-friendly web UI or a curl-friendly REST API compatible with Home Assistant.

---

## Architecture

Single Docker container, single port (default `8080`, configurable via `PORT` env var).

The Rust binary (Axum) does two things:
- Serves the REST API at `/api/*`
- Serves the SvelteKit static build for all other routes

**Requires `--network host`** — Wake-on-LAN sends UDP broadcasts that Docker bridge networking blocks. Host networking also gives direct access to IPMI IPs on the LAN.

**Single volume mount**: `/config` — holds both `config.yaml` and `servmgr.db`.

```
docker run \
  --network host \
  --cap-add NET_RAW \
  -e PORT=8080 \
  -v ./config:/config \
  servmgr
```

```
┌──────────────────────────────────────┐
│  Docker container (--network host)   │
│                                      │
│  ┌──────────┐   /api/*   ┌────────┐  │
│  │ SvelteKit│◄──────────►│  Axum  │  │
│  │ (static) │            │ (Rust) │  │
│  └──────────┘            └───┬────┘  │
│                              │       │
│              ┌───────────────┤       │
│              │               │       │
│         ┌────▼───┐   ┌───────▼────┐  │
│         │SQLite  │   │ Health     │  │
│         │/config/│   │ check +    │  │
│         │servmgr │   │ power      │  │
│         │.db     │   │ tasks      │  │
│         └────────┘   └────────────┘  │
└──────────────────────────────────────┘
```

**Crates:**
- `axum` — web framework
- `tokio` — async runtime
- `sqlx` (sqlite feature) — database
- `serde` + `serde_yaml` — config parsing
- `reqwest` — HTTP health checks
- `surge-ping` — ICMP ping (requires `CAP_NET_RAW`)
- `notify` — YAML file hot-reload on change

---

## Config File (`/config/config.yaml`)

Editable via file or the web UI. Hot-reloaded on change. On startup, cycles in the dependency graph are detected; affected servers get a `config_error` state (monitoring still runs, power actions are disabled).

On first run, if `/config/config.yaml` doesn't exist, servmgr creates a default empty config (`servers: []`) and starts normally. The user sees an empty dashboard and can add servers via the config editor in the UI.

```yaml
servers:
  - id: nas
    name: "NAS"
    hostname: "nas.local"
    power_on: wol
    mac: "aa:bb:cc:dd:ee:ff"
    power_off: ssh
    ssh_user: "thomas"
    ssh_key_path: "/config/id_rsa"   # or ssh_password: "secret"
    ssh_shutdown_cmd: "sudo systemctl poweroff"  # optional, default: "sudo shutdown -h now"
    health_checks:
      - type: ping
      - type: http
        url: "http://nas.local:8096"
      - type: tcp
        port: 22
    check_interval_secs: 30
    power_on_timeout_secs: 300       # optional, default: 300

  - id: homeserver
    name: "Home Server"
    hostname: "homeserver.local"
    power_on: ipmi
    ipmi_ip: "192.168.1.201"
    ipmi_user: "admin"
    ipmi_password: "secret"
    power_off: ipmi
    depends_on:
      - nas                          # homeserver needs nas to be up first
    health_checks:
      - type: ping
      - type: ipmi_power
    check_interval_secs: 60
```

**Power on methods:** `wol` (requires `mac`), `ipmi` (requires `ipmi_ip`, `ipmi_user`, `ipmi_password`)

**Power off methods:** `ssh` (requires `ssh_user` + `ssh_key_path` or `ssh_password`; optional `ssh_shutdown_cmd`, default `sudo shutdown -h now`), `ipmi`

**Health check types:** `ping`, `http` (requires `url`), `tcp` (requires `port`), `ssh` (TCP connect to port 22), `ipmi_power`

---

## REST API

No authentication (private network). Designed for curl and Home Assistant.

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/servers` | List all servers with current status |
| `GET` | `/api/servers/{id}` | Single server detail + status |
| `POST` | `/api/servers/{id}/poweron` | Increment counter; power on if 0→1 (async) |
| `POST` | `/api/servers/{id}/poweroff` | Decrement counter; power off if 1→0 (async) |
| `POST` | `/api/servers/{id}/forcepoweron` | Call WoL/IPMI directly, bypass counter |
| `POST` | `/api/servers/{id}/forcepoweroff` | Call SSH/IPMI directly, bypass counter |
| `GET` | `/api/servers/{id}/history` | Status history (`?from=&to=` ISO 8601) |
| `PUT` | `/api/servers/{id}/counter` | Manually set counter value (escape hatch) |
| `GET` | `/api/config` | Get raw YAML as text |
| `PUT` | `/api/config` | Replace YAML config (triggers hot-reload) |
| `GET` | `/api/events` | SSE stream — pushes status updates in real-time |

**Power on/off request body:**
```json
{
  "caller": "ha-bedroom-scene"
}
```

The `caller` field identifies who is requesting the action. Duplicate increments from the same caller are ignored (idempotent). The caller is tracked so that the corresponding `/poweroff` from the same caller correctly decrements.

**Counter override:**
```json
PUT /api/servers/nas/counter
{
  "value": 0
}
```

**Status response shape:**
```json
{
  "id": "nas",
  "name": "NAS",
  "hostname": "nas.local",
  "power_state": "on",
  "counter": 2,
  "callers": ["ha-bedroom-scene", "ha-movie-mode"],
  "status": "up",
  "checks": [
    { "type": "ping", "ok": true, "latency_ms": 2 },
    { "type": "http", "ok": true, "latency_ms": 45 },
    { "type": "tcp", "port": 22, "ok": true }
  ],
  "last_checked": "2026-06-27T11:30:00Z",
  "config_error": null
}
```

If a server has a dependency cycle, `config_error` contains a human-readable description (e.g. `"Cycle detected: nas → homeserver → nas"`), and power action endpoints return HTTP 409 with the same message.

---

## Power Management & Dependency Engine

### Reference Counter (Caller-Tracked)

Each server has a `counter` (int ≥ 0) stored in SQLite, a list of active `callers`, and a `power_state` (`off` | `pending_on` | `on` | `pending_off` | `failed`).

State transitions:
- `off` → `pending_on` when counter goes 0→1
- `pending_on` → `on` when health status first reaches `up`
- `pending_on` → `failed` when `power_on_timeout_secs` is exceeded
- `on` → `pending_off` when counter goes 1→0
- `pending_off` → `off` when health status reaches `down`

**Caller tracking:**
- **`/poweron`** with `caller`: if this caller already incremented, no-op (idempotent). Otherwise increment counter. If 0→1: set `power_state = pending_on`, start power-on sequence.
- **`/poweroff`** with `caller`: if this caller hasn't incremented, no-op. Otherwise remove caller, decrement counter. If 1→0: set `power_state = pending_off`, start power-off sequence.
- **`PUT /counter`**: manually override the counter value and clear all caller tracking. Emergency escape hatch for stuck states.
- **`/forcepoweron`** / **`/forcepoweroff`**: bypass counter entirely, call hardware directly.

### Dependency Chain — Power On

When server A (`depends_on: [B]`) is powered on:

1. Increment A's counter (0→1 triggers sequence)
2. Increment B's counter (propagate dependency; if 0→1, trigger B's power-on too)
3. Background task: send WoL/IPMI to B, then retry until B's health status = `up` or `power_on_timeout_secs` is exceeded
4. If B reaches `up`: send WoL/IPMI to A
5. If timeout exceeded: transition to `failed`, stop retrying, emit SSE event

### Dependency Chain — Power Off

When server A (`depends_on: [B]`) is powered off:

1. Decrement A's counter
2. If A's counter hits 0: send SSH/IPMI shutdown to A
3. Background task: wait until A's health status = `down`
4. Once A is `down`: decrement B's counter
5. If B's counter hits 0: send shutdown to B

This means B stays on as long as any server that depends on it still has counter > 0.

### Cycle Detection

On startup and on every config reload, the dependency graph is traversed (DFS). Servers involved in a cycle are flagged with `config_error`. They still appear in the UI and their health is still monitored — only power actions are disabled.

### Config Reload During In-Flight Operations

When config is reloaded, any in-flight power sequences for servers whose dependency graph changed are cancelled. Those servers transition back to their last stable state (`on` or `off` based on current health checks), and an SSE event is emitted explaining what happened. Servers with unchanged dependencies continue uninterrupted.

---

## Startup Reconciliation

On startup, servmgr immediately runs one health check cycle for all servers before serving requests. It reconciles `power_state` based on actual reachability:

- DB says `on` or `pending_on` but health checks say `down` → set `power_state = down`
- DB says `off` or `pending_off` but health checks say `up` → set `power_state = on`

Counters and caller lists are preserved as-is — they represent user intent. Only `power_state` is corrected to match reality. An SSE event is emitted for any reconciled server.

---

## Health Check Engine

One `tokio` task per server, running on `check_interval_secs`. All results written to SQLite (with ISO 8601 timestamps) and broadcast on the SSE stream.

| Check type | Implementation |
|------------|---------------|
| `ping` | ICMP via `surge-ping` (requires `CAP_NET_RAW`) |
| `http` / `https` | GET via `reqwest`, 2xx = ok |
| `tcp` | `TcpStream::connect` with timeout |
| `ssh` | TCP connect to port 22 |
| `ipmi_power` | Shell: `ipmitool chassis power status` |

**Overall server status:**
- `up` — all checks pass
- `degraded` — some checks pass
- `down` — all checks fail

---

## SSE Event Stream (`/api/events`)

On connection (or reconnection), the server immediately pushes the full current state of all servers as the first event. After that, incremental updates are pushed as changes occur.

```
event: full_state
data: [{"id": "nas", "status": "up", ...}, {"id": "homeserver", "status": "down", ...}]

event: update
data: {"id": "nas", "status": "degraded", ...}
```

---

## Frontend (SvelteKit + static adapter)

Mobile-first, responsive. One column on phone, card grid on desktop. No SSR — SvelteKit static adapter, served by the Rust binary.

**Dashboard (`/`)** — one card per server:
- Server name + hostname
- Status badge: `Up` (green) / `Degraded` (amber) / `Down` (red) / `Pending` (blue) / `Failed` (orange) / `Config Error` (orange warning)
- Individual check results with icons
- Counter value + active callers
- Power On / Power Off buttons (disabled + tooltip if `config_error`)
- Last checked timestamp
- Real-time updates via SSE (no polling)

**Server detail (`/servers/{id}`):**
- Health check breakdown
- Status history timeline chart
- Event log (went down at X, came up at Y)

**Config page (`/config`):**
- CodeMirror YAML editor
- Save button → `PUT /api/config`
- Validation errors shown inline

---

## Docker Build

Multi-stage Dockerfile:

1. **Node stage** — `npm run build` → static files
2. **Rust stage** — `cargo build --release` → binary
3. **Final stage** — `debian:bookworm-slim` with `ipmitool` installed; binary serves static files from a path baked in at compile time or configurable via env var

`ipmitool` must be present in the final image (Rust shells out to it for IPMI operations).

---

## Verification

- `cargo test` — unit tests for cycle detection, counter logic, caller tracking, config parsing, startup reconciliation
- `docker build` — confirm multi-stage build succeeds
- Manual: run container with `--network host --cap-add NET_RAW`, add two servers to config, verify health checks appear in UI, test poweron/poweroff counter increments via curl with caller tracking, test SSE updates in browser, verify reconnection sends full state
- Home Assistant: configure a REST command (POST) pointing to `/api/servers/{id}/poweron` with `{"caller": "homeassistant"}` and verify it triggers correctly
