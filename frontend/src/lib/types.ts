export type PowerState = 'off' | 'pending_on' | 'on' | 'pending_off' | 'failed';
export type ServerStatus = 'up' | 'degraded' | 'down';
export type HealthCheckType = 'ping' | 'http' | 'tcp' | 'ssh' | 'ipmi_power';

export interface CheckResult {
	type: HealthCheckType;
	ok: boolean;
	latency_ms: number | null;
	port?: number;
}

export interface ServerState {
	id: string;
	name: string;
	hostname: string;
	power_state: PowerState;
	counter: number;
	callers: string[];
	status: ServerStatus;
	checks: CheckResult[];
	last_checked: string | null;
	config_error: string | null;
	depends_on: string[];
}

export interface HistoryEntry {
	server_id: string;
	status: string;
	power_state: string;
	checks: CheckResult[];
	timestamp: string;
}

export interface PowerLogEntry {
	id: number;
	server_id: string;
	timestamp: string;
	command: string;
	success: boolean;
	message: string;
}
