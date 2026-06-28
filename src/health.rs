use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::types::{CheckResult, HealthCheckConfig, HealthCheckType, PowerOffMethod, PowerOnMethod, ServerConfig, ServerStatus};

pub fn compute_status(checks: &[CheckResult]) -> ServerStatus {
    if checks.is_empty() {
        return ServerStatus::Down;
    }
    let passing = checks.iter().filter(|c| c.ok).count();
    if passing == checks.len() {
        ServerStatus::Up
    } else if passing > 0 {
        ServerStatus::Degraded
    } else {
        ServerStatus::Down
    }
}

pub async fn run_all_checks(server: &ServerConfig) -> Vec<CheckResult> {
    let mut results = Vec::new();
    for check in &server.health_checks {
        let result = run_check(check, &server.hostname, server).await;
        results.push(result);
    }
    let uses_ipmi = server.power_on == PowerOnMethod::Ipmi || server.power_off == PowerOffMethod::Ipmi;
    let has_ipmi_check = server.health_checks.iter().any(|c| c.check_type == HealthCheckType::IpmiPower);
    if uses_ipmi && !has_ipmi_check {
        results.push(run_ipmi_power(server).await);
    }
    results
}

async fn run_check(
    check: &HealthCheckConfig,
    hostname: &str,
    server: &ServerConfig,
) -> CheckResult {
    match check.check_type {
        HealthCheckType::Ping => run_ping(hostname).await,
        HealthCheckType::Http => run_http(check.url.as_deref().unwrap_or("")).await,
        HealthCheckType::Tcp => run_tcp(hostname, check.port.unwrap_or(80)).await,
        HealthCheckType::Ssh => run_tcp(hostname, 22).await,
        HealthCheckType::IpmiPower => run_ipmi_power(server).await,
    }
}

async fn run_ping(hostname: &str) -> CheckResult {
    let start = Instant::now();

    let ok = match surge_ping::ping(
        hostname
            .parse()
            .unwrap_or_else(|_| resolve_hostname(hostname)),
        &[0u8; 8],
    )
    .await
    {
        Ok((_, duration)) => {
            debug!("Ping {hostname}: {duration:?}");
            true
        }
        Err(e) => {
            debug!("Ping {hostname} failed: {e}");
            false
        }
    };

    let latency = start.elapsed().as_millis() as u64;
    CheckResult {
        check_type: HealthCheckType::Ping,
        ok,
        latency_ms: Some(latency),
        port: None,
    }
}

fn resolve_hostname(hostname: &str) -> std::net::IpAddr {
    use std::net::ToSocketAddrs;
    format!("{hostname}:0")
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .map(|addr| addr.ip())
        .unwrap_or_else(|| std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0)))
}

async fn run_http(url: &str) -> CheckResult {
    let start = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let ok = match client.get(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(e) => {
            debug!("HTTP check {url} failed: {e}");
            false
        }
    };

    let latency = start.elapsed().as_millis() as u64;
    CheckResult {
        check_type: HealthCheckType::Http,
        ok,
        latency_ms: Some(latency),
        port: None,
    }
}

async fn run_tcp(hostname: &str, port: u16) -> CheckResult {
    let start = Instant::now();
    let addr = format!("{hostname}:{port}");

    let ok = match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => true,
        _ => {
            debug!("TCP check {addr} failed");
            false
        }
    };

    let latency = start.elapsed().as_millis() as u64;
    CheckResult {
        check_type: if port == 22 {
            HealthCheckType::Ssh
        } else {
            HealthCheckType::Tcp
        },
        ok,
        latency_ms: Some(latency),
        port: Some(port),
    }
}

async fn run_ipmi_power(server: &ServerConfig) -> CheckResult {
    let start = Instant::now();
    let ipmi_ip = server.ipmi_ip.as_deref().unwrap_or("");
    let ipmi_user = server.ipmi_user.as_deref().unwrap_or("admin");
    let ipmi_password = server.ipmi_password.as_deref().unwrap_or("");

    let output = Command::new("ipmitool")
        .args([
            "-I", "lanplus",
            "-H", ipmi_ip,
            "-U", ipmi_user,
            "-P", ipmi_password,
            "chassis", "power", "status",
        ])
        .output()
        .await;

    let ok = match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.contains("Chassis Power is on")
        }
        Err(e) => {
            warn!("IPMI power check failed: {e}");
            false
        }
    };

    let latency = start.elapsed().as_millis() as u64;
    CheckResult {
        check_type: HealthCheckType::IpmiPower,
        ok,
        latency_ms: Some(latency),
        port: None,
    }
}
