use sqlx::SqlitePool;
use tokio::process::Command;
use tracing::{info, warn};

use crate::db;
use crate::types::{PowerOffMethod, PowerOnMethod, ServerConfig};

pub async fn power_on(server: &ServerConfig, pool: &SqlitePool, server_id: &str) -> Result<(), String> {
    match server.power_on {
        PowerOnMethod::Wol => {
            let (result, output) = send_wol(server.mac.as_deref().unwrap_or(""), server.wol_broadcast.as_deref());
            db::insert_power_log(pool, server_id, "wol", result.is_ok(), &output).await.ok();
            result
        }
        PowerOnMethod::Ipmi => {
            let (result, output) = ipmi_power_on(server).await;
            db::insert_power_log(pool, server_id, "ipmi_on", result.is_ok(), &output).await.ok();
            result
        }
    }
}

pub async fn power_off(server: &ServerConfig, pool: &SqlitePool, server_id: &str) -> Result<(), String> {
    match server.power_off {
        PowerOffMethod::Ssh => {
            let (result, output) = ssh_shutdown(server).await;
            db::insert_power_log(pool, server_id, "ssh_off", result.is_ok(), &output).await.ok();
            result
        }
        PowerOffMethod::Ipmi => {
            let (result, output) = ipmi_power_off(server).await;
            db::insert_power_log(pool, server_id, "ipmi_off", result.is_ok(), &output).await.ok();
            result
        }
    }
}

fn send_wol(mac: &str, broadcast: Option<&str>) -> (Result<(), String>, String) {
    let mac_bytes = match parse_mac(mac) {
        Ok(b) => b,
        Err(e) => return (Err(e.clone()), e),
    };
    let mut magic_packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        magic_packet.extend_from_slice(&mac_bytes);
    }
    let socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => { let m = format!("Failed to bind UDP socket: {e}"); return (Err(m.clone()), m); }
    };
    if let Err(e) = socket.set_broadcast(true) {
        let m = format!("Failed to set broadcast: {e}"); return (Err(m.clone()), m);
    }

    // Always send to 255.255.255.255 and also to the directed broadcast if configured
    let mut targets = vec!["255.255.255.255:9".to_string()];
    if let Some(addr) = broadcast {
        let directed = if addr.contains(':') { addr.to_string() } else { format!("{addr}:9") };
        if directed != targets[0] {
            targets.push(directed);
        }
    }

    let mut log_lines = Vec::new();
    let mut last_err: Option<String> = None;

    for target in &targets {
        match socket.send_to(&magic_packet, target.as_str()) {
            Ok(n) => {
                info!("Sent WoL packet to {mac} via {target}");
                log_lines.push(format!("Sent {n}-byte magic packet → {target} (MAC: {mac})"));
                last_err = None;
            }
            Err(e) => {
                let m = format!("Failed to send to {target}: {e}");
                log_lines.push(m.clone());
                last_err = Some(m);
            }
        }
    }

    let log = log_lines.join("\n");
    if last_err.is_some() && log_lines.len() == targets.len() && targets.len() == 1 {
        // Only target failed
        return (Err(last_err.unwrap()), log);
    }
    (Ok(()), log)
}

fn parse_mac(mac: &str) -> Result<[u8; 6], String> {
    let parts: Vec<&str> = mac.split(':').collect();
    if parts.len() != 6 {
        return Err(format!("Invalid MAC address: {mac}"));
    }
    let mut bytes = [0u8; 6];
    for (i, part) in parts.iter().enumerate() {
        bytes[i] = u8::from_str_radix(part, 16)
            .map_err(|_| format!("Invalid MAC address byte: {part}"))?;
    }
    Ok(bytes)
}

async fn ipmi_power_on(server: &ServerConfig) -> (Result<(), String>, String) {
    let ip = server.ipmi_ip.as_deref().unwrap_or("");
    let user = server.ipmi_user.as_deref().unwrap_or("admin");
    let password = server.ipmi_password.as_deref().unwrap_or("");

    let out = match Command::new("ipmitool")
        .args(["-I", "lanplus", "-H", ip, "-U", user, "-P", password, "chassis", "power", "on"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => { let m = format!("Failed to run ipmitool: {e}"); return (Err(m.clone()), m); }
    };

    let combined = combined_output(&out.stdout, &out.stderr);
    if out.status.success() {
        info!("IPMI power on sent to {ip}");
        (Ok(()), combined)
    } else {
        (Err(format!("ipmitool exited {}", out.status)), combined)
    }
}

async fn ipmi_power_off(server: &ServerConfig) -> (Result<(), String>, String) {
    let ip = server.ipmi_ip.as_deref().unwrap_or("");
    let user = server.ipmi_user.as_deref().unwrap_or("admin");
    let password = server.ipmi_password.as_deref().unwrap_or("");

    let out = match Command::new("ipmitool")
        .args(["-I", "lanplus", "-H", ip, "-U", user, "-P", password, "chassis", "power", "off"])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => { let m = format!("Failed to run ipmitool: {e}"); return (Err(m.clone()), m); }
    };

    let combined = combined_output(&out.stdout, &out.stderr);
    if out.status.success() {
        info!("IPMI power off sent to {ip}");
        (Ok(()), combined)
    } else {
        (Err(format!("ipmitool exited {}", out.status)), combined)
    }
}

async fn ssh_shutdown(server: &ServerConfig) -> (Result<(), String>, String) {
    let user = server.ssh_user.as_deref().unwrap_or("root");
    let hostname = &server.hostname;
    let cmd = server.ssh_shutdown_cmd.as_deref().unwrap_or("sudo shutdown -h now");

    let mut args = vec!["-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10"];
    let use_password = server.ssh_password.is_some();
    if !use_password {
        if let Some(key_path) = &server.ssh_key_path {
            args.extend_from_slice(&["-i", key_path.as_str()]);
        }
    }
    let target = format!("{user}@{hostname}");
    args.push(&target);

    // When authenticating with a password, pipe it into sudo -S so the remote
    // sudo prompt is satisfied without requiring a TTY.
    let sudo_wrapped;
    let effective_cmd = if use_password && cmd.contains("sudo") && !cmd.contains("sudo -S") {
        let password = server.ssh_password.as_deref().unwrap_or("");
        let escaped = password.replace('\'', r"'\''");
        sudo_wrapped = format!("echo '{}' | {}", escaped, cmd.replacen("sudo", "sudo -S", 1));
        sudo_wrapped.as_str()
    } else {
        cmd
    };
    args.push(effective_cmd);

    let out = if use_password {
        let password = server.ssh_password.as_deref().unwrap_or("");
        match Command::new("sshpass").arg("-p").arg(password).arg("ssh").args(&args).output().await {
            Ok(o) => o,
            Err(e) => { let m = format!("Failed to run sshpass/ssh: {e}"); return (Err(m.clone()), m); }
        }
    } else {
        match Command::new("ssh").args(&args).output().await {
            Ok(o) => o,
            Err(e) => { let m = format!("Failed to run ssh: {e}"); return (Err(m.clone()), m); }
        }
    };

    let combined = combined_output(&out.stdout, &out.stderr);

    // SSH shutdown often closes before returning success — treat stderr about closed connections as ok
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("closed by remote host") || stderr.contains("Connection to") {
            info!("SSH shutdown sent to {hostname}");
            return (Ok(()), combined);
        }
        warn!("SSH shutdown may have failed for {hostname}: {stderr}");
    }

    info!("SSH shutdown sent to {hostname}");
    (Ok(()), combined)
}

fn combined_output(stdout: &[u8], stderr: &[u8]) -> String {
    let out = String::from_utf8_lossy(stdout).trim().to_string();
    let err = String::from_utf8_lossy(stderr).trim().to_string();
    match (out.is_empty(), err.is_empty()) {
        (false, false) => format!("{out}\n{err}"),
        (false, true)  => out,
        (true,  false) => err,
        (true,  true)  => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mac_valid() {
        let result = parse_mac("aa:bb:cc:dd:ee:ff");
        assert_eq!(result.unwrap(), [0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    }

    #[test]
    fn test_parse_mac_invalid() {
        assert!(parse_mac("invalid").is_err());
        assert!(parse_mac("aa:bb:cc:dd:ee").is_err());
        assert!(parse_mac("gg:bb:cc:dd:ee:ff").is_err());
    }
}
