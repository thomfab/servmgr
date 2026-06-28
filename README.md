# servmgr

Home network server manager — a self-hosted dashboard for powering servers on and off on demand, with dependency tracking, health monitoring, and reference counting.

## What it does

- **Reference-counted power management**: servers stay on as long as at least one caller needs them, and shut down automatically when the counter reaches zero
- **Dependency graph**: declare that server B requires server A — servmgr starts A first and shuts it down last
- **Health checks**: ping, HTTP, TCP port, SSH, and IPMI power status — the dashboard updates in real time via SSE
- **Multiple power methods**: Wake-on-LAN (with optional directed broadcast for VMs), IPMI, and SSH shutdown
- **Fast transition polling**: when a power-on or power-off is triggered, health checks run every 3 s until the transition completes, then revert to the configured interval
- **Config editor**: full YAML config editable in the browser — no need to SSH into the server

## Stack

| Layer    | Technology                          |
|----------|-------------------------------------|
| Backend  | Rust · axum · sqlx / SQLite · tokio |
| Frontend | SvelteKit · Svelte 5 (runes) · static adapter |
| Runtime  | Docker · debian:bookworm-slim       |

## Quick start

```bash
# Pull the latest image
docker pull ghcr.io/thomfab/servmgr:latest

# Run (host network required for Wake-on-LAN and ping)
docker run -d \
  --name servmgr \
  --network host \
  --cap-add NET_RAW \
  -v /path/to/config:/config \
  ghcr.io/thomfab/servmgr:latest
```

Open `http://<host>:8080` in your browser.

To bind a specific port instead of using host networking:

```bash
docker run -d \
  --name servmgr \
  -p 8080:8080 \
  -v /path/to/config:/config \
  ghcr.io/thomfab/servmgr:latest
```

> **Note**: Wake-on-LAN and ICMP ping require `--network host` and `--cap-add NET_RAW`. Without host networking, only HTTP/TCP/SSH health checks and IPMI power control work.

## Configuration

On first run, create `/path/to/config/config.yaml`. You can also edit the config directly in the browser under the **Config** tab.

```yaml
servers:
  - id: nas
    name: "NAS"
    hostname: "nas.local"
    power_on: wol
    mac: "aa:bb:cc:dd:ee:ff"
    # Optional: directed broadcast for ESXi/Proxmox VMs
    # wol_broadcast: "192.168.1.255"
    power_off: ssh
    ssh_user: "admin"
    ssh_password: "secret"
    ssh_shutdown_cmd: "sudo poweroff"
    health_checks:
      - type: ping
      - type: tcp
        port: 445

  - id: workstation
    name: "Workstation"
    hostname: "pc.local"
    power_on: wol
    mac: "11:22:33:44:55:66"
    power_off: ssh
    ssh_user: "user"
    ssh_key_path: "/config/id_rsa"
    ssh_shutdown_cmd: "sudo shutdown -h now"
    depends_on:
      - nas
    health_checks:
      - type: ping
      - type: ssh

  - id: server
    name: "Server"
    hostname: "server.local"
    power_on: ipmi
    power_off: ipmi
    ipmi_ip: "server-ipmi.local"
    ipmi_user: "admin"
    ipmi_password: "secret"
    health_checks:
      - type: ping
      - type: ipmi_power
```

### Config fields

| Field | Description |
|-------|-------------|
| `id` | Unique identifier (used in URLs and dependency references) |
| `name` | Display name |
| `hostname` | Hostname or IP used for health checks |
| `power_on` | `wol` or `ipmi` |
| `mac` | MAC address (WoL only) |
| `wol_broadcast` | Directed broadcast address (optional, WoL only) |
| `power_off` | `ssh` or `ipmi` |
| `ssh_user` | SSH username |
| `ssh_key_path` | Path to SSH private key inside the container |
| `ssh_password` | SSH password (alternative to key) |
| `ssh_shutdown_cmd` | Remote command to run (default: `sudo shutdown -h now`) |
| `ipmi_ip` | IPMI/BMC IP address |
| `ipmi_user` | IPMI username |
| `ipmi_password` | IPMI password |
| `depends_on` | List of server IDs that must be running first |
| `health_checks` | List of checks (see below) |
| `check_interval_secs` | Seconds between health checks (default: 30) |
| `power_on_timeout_secs` | Seconds before a pending-on transitions to failed (default: 300) |

### Health check types

| Type | Description |
|------|-------------|
| `ping` | ICMP echo |
| `http` | HTTP GET — requires `url` field |
| `tcp` | TCP connect — requires `port` field |
| `ssh` | SSH handshake on port 22 |
| `ipmi_power` | IPMI chassis power status via `ipmitool` |

## Building locally

```bash
docker build -t servmgr:latest .
```

Requires Docker with BuildKit. The build is multi-stage: Node 22 for the SvelteKit frontend, then Rust 1.87 for the backend, assembled into a `debian:bookworm-slim` final image.

## License

Apache 2.0 — see [LICENSE](LICENSE).
