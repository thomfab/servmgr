# User Guide

## What is servmgr?

servmgr is a home network server manager. It monitors server health and lets you power servers on and off from a web UI or REST API, with support for power dependencies between servers.

## Quick Start

### Docker Compose (recommended)

Create a `docker-compose.yml`:

```yaml
services:
  servmgr:
    image: ghcr.io/thomfab/servmgr:latest
    container_name: servmgr
    network_mode: host          # required for Wake-on-LAN and ICMP ping
    cap_add:
      - NET_RAW                 # required for raw socket ping
    volumes:
      - ./config:/config        # config.yaml and SSH keys live here
    restart: unless-stopped
```

Then:

```bash
mkdir -p config
docker compose up -d
```

Open http://localhost:8080, go to the **Config** tab, and add your servers.

> **Port binding instead of host networking** — if you don't need WoL or ping, you can drop `network_mode: host` and expose the port explicitly:
> ```yaml
>     ports:
>       - "8080:8080"
> ```

### Docker run

```bash
docker run -d \
  --name servmgr \
  --network host \
  --cap-add NET_RAW \
  -v ./config:/config \
  --restart unless-stopped \
  ghcr.io/thomfab/servmgr:latest
```

### Example Configuration

```yaml
servers:
  - id: nas
    name: "NAS"
    hostname: "nas.local"
    power_on: wol
    mac: "aa:bb:cc:dd:ee:ff"
    power_off: ssh
    ssh_user: "thomas"
    ssh_key_path: "/config/id_rsa"
    health_checks:
      - type: ping
      - type: http
        url: "http://nas.local:8096"
      - type: tcp
        port: 22
    check_interval_secs: 30

  - id: homeserver
    name: "Home Server"
    hostname: "homeserver.local"
    power_on: ipmi
    ipmi_ip: "192.168.1.201"
    ipmi_user: "admin"
    ipmi_password: "secret"
    power_off: ipmi
    depends_on:
      - nas
    health_checks:
      - type: ping
      - type: ipmi_power
    check_interval_secs: 60
```

## Configuration Reference

### Server fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | Unique identifier (used in URLs) |
| `name` | Yes | Display name |
| `hostname` | Yes | Network hostname or IP |
| `power_on` | Yes | `wol` or `ipmi` |
| `power_off` | Yes | `ssh` or `ipmi` |
| `health_checks` | Yes | Array of check definitions |
| `mac` | If wol | MAC address for Wake-on-LAN |
| `wol_broadcast` | No | Directed broadcast address for WoL (e.g. `192.168.1.255`). Recommended when running behind a hypervisor (ESXi, Proxmox) where `255.255.255.255` may not leave the virtual switch. Both addresses are tried when set. |
| `ipmi_ip` | If ipmi | IPMI BMC IP address |
| `ipmi_user` | If ipmi | IPMI username |
| `ipmi_password` | If ipmi | IPMI password |
| `ssh_user` | If ssh | SSH username for shutdown |
| `ssh_key_path` | If ssh | Path to SSH private key |
| `ssh_password` | If ssh | SSH password (alternative to key) |
| `ssh_shutdown_cmd` | No | Shutdown command (default: `sudo shutdown -h now`) |
| `depends_on` | No | Array of server IDs this server depends on |
| `check_interval_secs` | No | Health check interval (default: 30) |
| `power_on_timeout_secs` | No | Power on timeout before marking failed (default: 300) |

### Health check types

| Type | Extra fields | What it checks |
|------|-------------|----------------|
| `ping` | — | ICMP echo reply |
| `http` | `url` | HTTP GET returns 2xx |
| `tcp` | `port` | TCP connection succeeds |
| `ssh` | — | TCP connect to port 22 |
| `ipmi_power` | — | IPMI reports chassis power on |

## Web UI

### Dashboard

The main page shows all servers as cards with:
- Server name and hostname
- Power state badge (On/Off/Starting/Stopping/Failed)
- Health check results with latency
- Reference counter
- Power On / Power Off buttons

Cards update in real-time via Server-Sent Events — no page refresh needed.

### Server Detail

Click a server name to see:
- Full health check breakdown
- Status history timeline (last 24 hours)
- Active callers list
- Power controls

### Config Editor

Edit the YAML config directly in the browser. Changes take effect immediately on save (hot-reload).

## Home Assistant Integration

### REST Commands

Add to `configuration.yaml` (or split across packages). Replace `servmgr-host` with the hostname or IP of the machine running servmgr, and `nas` with your server ID.

```yaml
rest_command:
  # Increment reference counter — server powers on when counter goes 0→1
  nas_powerinc:
    url: "http://servmgr-host:8080/api/servers/nas/powerinc"
    method: POST
    content_type: "application/json"
    payload: '{"caller": "homeassistant"}'

  # Decrement reference counter — server powers off when counter goes 1→0
  nas_powerdec:
    url: "http://servmgr-host:8080/api/servers/nas/powerdec"
    method: POST
    content_type: "application/json"
    payload: '{"caller": "homeassistant"}'

  # Force power on (WoL/IPMI), bypasses the counter
  nas_poweron:
    url: "http://servmgr-host:8080/api/servers/nas/poweron"
    method: POST

  # Force power off (SSH/IPMI), bypasses the counter
  nas_poweroff:
    url: "http://servmgr-host:8080/api/servers/nas/poweroff"
    method: POST
```

The caller-tracking system ensures that if multiple HA automations request a server to be on, it stays on until all of them release it via `powerdec`.

### Sensors

Add a REST sensor that polls the server state, then derive template sensors for the counter and status LED:

```yaml
sensor:
  - platform: rest
    name: "servmgr_nas"
    resource: "http://servmgr-host:8080/api/servers/nas"
    json_attributes:
      - counter
      - power_state
      - health
    value_template: "{{ value_json.power_state }}"
    scan_interval: 30

template:
  - sensor:
      - name: "NAS Status"
        unique_id: nas_status_led
        state: "{{ state_attr('sensor.servmgr_nas', 'power_state') }}"
        icon: mdi:circle
        # Grey = off, orange = on but degraded, green = all checks ok
        icon_color: >
          {% set ps = state_attr('sensor.servmgr_nas', 'power_state') %}
          {% set health = state_attr('sensor.servmgr_nas', 'health') %}
          {% if ps in ['off', 'pending_off', none] %}
            grey
          {% elif ps == 'on' and health == 'up' %}
            green
          {% else %}
            orange
          {% endif %}

      - name: "NAS Counter"
        unique_id: nas_counter
        state: "{{ state_attr('sensor.servmgr_nas', 'counter') | int(0) }}"
        unit_of_measurement: "refs"
        icon: mdi:counter
```

### Dashboard Card

Add to a Lovelace dashboard as raw YAML:

```yaml
type: vertical-stack
cards:
  - type: entities
    title: NAS
    entities:
      - entity: sensor.nas_status
        name: Status
      - entity: sensor.nas_counter
        name: Counter
  - type: horizontal-stack
    cards:
      - type: button
        name: Inc
        icon: mdi:plus-circle-outline
        tap_action:
          action: call-service
          service: rest_command.nas_powerinc
      - type: button
        name: Dec
        icon: mdi:minus-circle-outline
        tap_action:
          action: call-service
          service: rest_command.nas_powerdec
      - type: button
        name: Force On
        icon: mdi:power-plug
        tap_action:
          action: call-service
          service: rest_command.nas_poweron
      - type: button
        name: Force Off
        icon: mdi:power-plug-off
        tap_action:
          action: call-service
          service: rest_command.nas_poweroff
```

The status LED icon color tracks three states: **grey** when the server is off or shutting down, **orange** when the server is on but at least one health probe is failing, **green** when the server is on and all health checks pass.

## Reference Counter

The reference counter tracks how many "reasons" a server should be powered on. Each caller (HA automation, web UI click, dependency) adds one.

- Server powers on when counter goes from 0 to 1
- Server powers off when counter goes from 1 to 0
- Counter > 1 means multiple things want it on — it stays on

If the counter gets stuck (e.g., an automation crashed without sending `powerdec`), use the counter override:

```bash
curl -X PUT http://localhost:8080/api/servers/nas/counter \
  -H "Content-Type: application/json" \
  -d '{"value": 0}'
```

## Dependencies

When server A `depends_on: [B]`:

- **Power on A**: B is automatically powered on first. A only starts after B is healthy.
- **Power off A**: A shuts down first. B's counter is decremented after A is confirmed down. B only shuts down if nothing else needs it.

Circular dependencies are detected and flagged — affected servers show a config error and power actions are disabled.

## Troubleshooting

### Server stuck in "Starting"

The server couldn't reach a healthy state within `power_on_timeout_secs`. Check:
- Is the server physically reachable?
- Are health checks correctly configured?
- For WoL: is the server on the same L2 network segment?

Reset with: `PUT /api/servers/{id}/counter` with `{"value": 0}`.

### WoL not working

- Ensure `--network host` is used (WoL broadcasts don't work through Docker bridge networking)
- Ensure `--cap-add NET_RAW` is set
- Verify the MAC address is correct
- Check that WoL is enabled in the server's BIOS/UEFI

### IPMI errors

- Verify IPMI credentials and IP
- Ensure `ipmitool` is accessible in the container
- Test manually: `ipmitool -I lanplus -H <ip> -U <user> -P <pass> chassis power status`
