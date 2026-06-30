export type ServerStatus = 'off' | 'on' | 'turning_on' | 'turning_off' | 'degraded';
export type HealthCheckType = 'ping' | 'http' | 'tcp' | 'ssh' | 'ipmi_power';

export interface CheckResult {
	type: HealthCheckType;
	ok: boolean;
	latency_ms: number | null;
	port?: number;
	label?: string;
}

export interface ServerState {
	id: string;
	name: string;
	hostname: string;
	counter: number;
	callers: string[];
	status: ServerStatus;
	power_timeout: number;
	checks: CheckResult[];
	last_checked: string | null;
	config_error: string | null;
	depends_on: string[];
}

export interface HistoryEntry {
	server_id: string;
	status: ServerStatus;
	checks: CheckResult[];
	counter: number;
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
