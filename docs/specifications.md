# Specifications

This document describes the behavior of servmgr. For the design rationale and original spec, see `../spec/main.md`.

## REST API

Base path: `/api`

### GET /api/servers

Returns all configured servers with current status.

**Response**: `200 OK` — JSON array of server state objects.

### GET /api/servers/{id}

Returns a single server's state.

**Response**: `200 OK` or `404 Not Found`.

### POST /api/servers/{id}/powerinc

Increment the reference counter for a server. If counter transitions 0→1, starts the power-on sequence (including dependencies).

**Request body**:
```json
{ "caller": "ha-bedroom-scene" }
```

**Idempotency**: If the same caller has already called powerinc, this is a no-op.

**Response**: `200 OK` with server state, or `409 Conflict` if server has a config error (cycle).

### POST /api/servers/{id}/powerdec

Decrement the reference counter. If counter transitions 1→0, starts power-off sequence and cascades to dependencies.

**Request body**:
```json
{ "caller": "ha-bedroom-scene" }
```

**Response**: `200 OK` with server state, or `409 Conflict` on config error.

### POST /api/servers/{id}/poweron

Send power-on command (WoL/IPMI) directly, bypassing the reference counter.

**Response**: `200 OK` with server state.

### POST /api/servers/{id}/poweroff

Send power-off command (SSH/IPMI) directly, bypassing the reference counter.

**Response**: `200 OK` with server state.

### GET /api/servers/{id}/history

Get status history for a server within a time range.

**Query parameters**:
- `from` — ISO 8601 timestamp (default: 24 hours ago)
- `to` — ISO 8601 timestamp (default: now)

**Response**: `200 OK` — JSON array of history entries.

### PUT /api/servers/{id}/counter

Manually override the counter value and clear all caller tracking.

**Request body**:
```json
{ "value": 0 }
```

**Response**: `200 OK` with server state.

### GET /api/config

Returns the raw YAML config file content as `text/plain`.

### PUT /api/config

Replace the config file with the request body (must be valid YAML).

**Response**: `204 No Content` on success, `400 Bad Request` with error details on invalid YAML.

### GET /api/events

Server-Sent Events stream.

**First event** on connect: `full_state` with all server states.
**Subsequent events**: `update` with individual server state changes.
**Additional events**: `config_reloaded` when config changes affect a server.

## Power State Machine

States: `off`, `pending_on`, `on`, `pending_off`, `failed`

Transitions:
- `off` → `pending_on`: counter goes 0→1
- `pending_on` → `on`: all health checks pass
- `pending_on` → `failed`: `power_on_timeout_secs` exceeded
- `on` → `pending_off`: counter goes 1→0
- `pending_off` → `off`: all health checks fail (server confirmed down)

## Health Check Behavior

Checks run at `check_interval_secs` (per-server, default 30).

**Overall status** is computed only from checks where `counts_toward_status` is `true` (default `true`; see below):
- `up`: all counting checks pass
- `degraded`: some counting checks pass, some fail
- `down`: no counting checks pass

**Check types**:
- `ping`: ICMP echo via surge-ping. Always runs automatically, even if not listed in `health_checks`, and always counts toward status — it can't be removed or excluded.
- `http`/`https`: GET request, 2xx = pass
- `tcp`: TCP connect with 5s timeout
- `ssh`: TCP connect to port 22
- `ipmi_power`: `ipmitool chassis power status`, "on" = pass. Auto-added (and always counts) if `power_on`/`power_off` is `ipmi` and no explicit `ipmi_power` check is configured.

**`counts_toward_status`** (per check, default `true`): when `false`, the check still runs and its result is still shown, but it's excluded from the overall status calculation above — useful for a check you want visibility into without letting it gate the server's Up/Degraded/Down state.

## Dependency Behavior

### Power On

When server A depends on B:
1. B's counter is incremented with caller `dep:A`
2. B is powered on first
3. System waits until B is healthy — reacting to B's health-check updates as they happen rather than polling on a fixed interval, bounded by A's `power_timeout_secs`
4. Then A is powered on

### Power Off

When server A depends on B and A is powered off:
1. A is shut down first
2. System waits until A is confirmed down
3. Then B's counter is decremented (caller `dep:A` removed)
4. If B's counter reaches 0, B is also shut down

### Cycles

Detected via DFS on startup and every config reload. Affected servers get `config_error` set and power actions return HTTP 409.

## Startup Behavior

1. Load or create default config
2. Validate config (cycle detection)
3. Initialize/migrate SQLite database
4. Run one immediate health check cycle for all servers
5. Reconcile `power_state` with actual health (without touching counters)
6. Start periodic health check tasks
7. Start config file watcher
8. Begin serving HTTP

## Config Hot-Reload

When `/config/config.yaml` changes (file system watch):
1. Parse and validate new config
2. Cancel in-flight power sequences for servers with changed dependencies
3. Restart all health check tasks with updated intervals
4. Emit SSE events for affected servers
