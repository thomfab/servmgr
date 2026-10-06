use std::time::{Duration, Instant};

use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::types::{CheckResult, HealthCheckConfig, HealthCheckType, HealthStatus, PowerOffMethod, PowerOnMethod, ServerConfig};

pub fn compute_status(checks: &[CheckResult]) -> HealthStatus {
    let counting: Vec<&CheckResult> = checks.iter().filter(|c| c.counts_toward_status).collect();
    if counting.is_empty() {
        return HealthStatus::Down;
    }
    let passing = counting.iter().filter(|c| c.ok).count();
    if passing == counting.len() {
        HealthStatus::Up
    } else if passing > 0 {
        HealthStatus::Degraded
    } else {
        HealthStatus::Down
    }
}

pub async fn run_all_checks(server: &ServerConfig) -> Vec<CheckResult> {
    // Ping always runs and always counts, regardless of what's configured —
    // any explicit `type: ping` entry (from an older config) is inert below.
    let mut results = vec![run_ping(&server.hostname).await];

    for check in &server.health_checks {
        if check.check_type == HealthCheckType::Ping {
            continue;
        }
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
    let mut result = match check.check_type {
        HealthCheckType::Ping => run_ping(hostname).await,
        HealthCheckType::Http => run_http(check.url.as_deref().unwrap_or("")).await,
        HealthCheckType::Tcp => run_tcp(hostname, check.port.unwrap_or(80)).await,
        HealthCheckType::Ssh => run_tcp(hostname, 22).await,
        HealthCheckType::IpmiPower => run_ipmi_power(server).await,
    };
    result.label = check.label.clone();
    result.counts_toward_status = check.counts_toward_status;
    result
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
        label: None,
        counts_toward_status: true,
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
        label: None,
        counts_toward_status: true,
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
        label: None,
        counts_toward_status: true,
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
        label: None,
        counts_toward_status: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_server(health_checks: Vec<HealthCheckConfig>) -> ServerConfig {
        ServerConfig {
            id: "s".into(),
            name: "s".into(),
            hostname: "127.0.0.1".into(),
            power_on: PowerOnMethod::Wol,
            mac: None,
            wol_broadcast: None,
            power_off: PowerOffMethod::Ssh,
            ssh_user: None,
            ssh_key_path: None,
            ssh_password: None,
            ssh_shutdown_cmd: None,
            ipmi_ip: None,
            ipmi_user: None,
            ipmi_password: None,
            depends_on: vec![],
            health_checks,
            check_interval_secs: 30,
            power_timeout_secs: 300,
        }
    }

    fn result(check_type: HealthCheckType, ok: bool, counts: bool) -> CheckResult {
        CheckResult { check_type, ok, latency_ms: None, port: None, label: None, counts_toward_status: counts }
    }

    #[test]
    fn compute_status_only_weighs_counting_checks() {
        let checks = vec![
            result(HealthCheckType::Ping, true, true),
            result(HealthCheckType::Http, false, false),
        ];
        assert_eq!(compute_status(&checks), HealthStatus::Up);
    }

    #[test]
    fn compute_status_is_down_when_no_check_counts() {
        let checks = vec![result(HealthCheckType::Http, true, false)];
        assert_eq!(compute_status(&checks), HealthStatus::Down);
    }

    #[tokio::test]
    async fn run_all_checks_injects_ping_even_when_not_configured() {
        let server = make_server(vec![]);
        let results = tokio::time::timeout(Duration::from_secs(5), run_all_checks(&server))
            .await
            .expect("run_all_checks should not hang");

        let ping_results: Vec<_> = results.iter().filter(|r| r.check_type == HealthCheckType::Ping).collect();
        assert_eq!(ping_results.len(), 1, "ping must be injected even when health_checks is empty");
        assert!(ping_results[0].counts_toward_status, "the implicit ping always counts toward status");
    }

    #[tokio::test]
    async fn run_all_checks_ignores_a_legacy_explicit_ping_entry() {
        let server = make_server(vec![HealthCheckConfig {
            check_type: HealthCheckType::Ping,
            url: None,
            port: None,
            label: None,
            counts_toward_status: true,
        }]);
        let results = tokio::time::timeout(Duration::from_secs(5), run_all_checks(&server))
            .await
            .expect("run_all_checks should not hang");

        let ping_count = results.iter().filter(|r| r.check_type == HealthCheckType::Ping).count();
        assert_eq!(ping_count, 1, "an explicit legacy ping entry must not duplicate the implicit one");
    }

    #[tokio::test]
    async fn run_all_checks_propagates_counts_toward_status_from_config() {
        let server = make_server(vec![HealthCheckConfig {
            check_type: HealthCheckType::Tcp,
            url: None,
            port: Some(1),
            label: None,
            counts_toward_status: false,
        }]);
        let results = tokio::time::timeout(Duration::from_secs(5), run_all_checks(&server))
            .await
            .expect("run_all_checks should not hang");

        let tcp = results.iter().find(|r| r.check_type == HealthCheckType::Tcp).unwrap();
        assert!(!tcp.counts_toward_status, "counts_toward_status must be copied from the check's config");
    }
}
