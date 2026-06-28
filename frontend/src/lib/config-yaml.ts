interface ServerEntry {
	id: string;
	name: string;
	hostname: string;
	power_on: string;
	mac?: string;
	wol_broadcast?: string;
	power_off: string;
	ssh_user?: string;
	ssh_key_path?: string;
	ssh_password?: string;
	ssh_shutdown_cmd?: string;
	ipmi_ip?: string;
	ipmi_user?: string;
	ipmi_password?: string;
	depends_on?: string[];
	health_checks: { type: string; url?: string; port?: number }[];
	check_interval_secs?: number;
	power_on_timeout_secs?: number;
}

export function serversToYaml(entries: ServerEntry[]): string {
	if (entries.length === 0) return 'servers: []\n';

	let yaml = 'servers:\n';
	for (const s of entries) {
		yaml += `  - id: ${s.id}\n`;
		yaml += `    name: "${s.name}"\n`;
		yaml += `    hostname: "${s.hostname}"\n`;
		yaml += `    power_on: ${s.power_on}\n`;
		if (s.mac) yaml += `    mac: "${s.mac}"\n`;
		if (s.wol_broadcast) yaml += `    wol_broadcast: "${s.wol_broadcast}"\n`;
		yaml += `    power_off: ${s.power_off}\n`;
		if (s.ssh_user) yaml += `    ssh_user: "${s.ssh_user}"\n`;
		if (s.ssh_key_path) yaml += `    ssh_key_path: "${s.ssh_key_path}"\n`;
		if (s.ssh_password) yaml += `    ssh_password: "${s.ssh_password}"\n`;
		if (s.ssh_shutdown_cmd) yaml += `    ssh_shutdown_cmd: "${s.ssh_shutdown_cmd}"\n`;
		if (s.ipmi_ip) yaml += `    ipmi_ip: "${s.ipmi_ip}"\n`;
		if (s.ipmi_user) yaml += `    ipmi_user: "${s.ipmi_user}"\n`;
		if (s.ipmi_password) yaml += `    ipmi_password: "${s.ipmi_password}"\n`;
		if (s.depends_on && s.depends_on.length > 0) {
			yaml += `    depends_on:\n`;
			for (const dep of s.depends_on) {
				yaml += `      - ${dep}\n`;
			}
		}
		if (s.health_checks.length > 0) {
			yaml += `    health_checks:\n`;
			for (const check of s.health_checks) {
				yaml += `      - type: ${check.type}\n`;
				if (check.url) yaml += `        url: "${check.url}"\n`;
				if (check.port) yaml += `        port: ${check.port}\n`;
			}
		}
		if (s.check_interval_secs && s.check_interval_secs !== 30) {
			yaml += `    check_interval_secs: ${s.check_interval_secs}\n`;
		}
		if (s.power_on_timeout_secs && s.power_on_timeout_secs !== 300) {
			yaml += `    power_on_timeout_secs: ${s.power_on_timeout_secs}\n`;
		}
	}
	return yaml;
}

export function parseConfigYaml(text: string): { servers: ServerEntry[] } {
	const lines = text.split('\n');
	const result: ServerEntry[] = [];
	let current: any = null;
	let inHealthChecks = false;
	let currentCheck: any = null;

	for (const line of lines) {
		const trimmed = line.trimStart();
		const indent = line.length - trimmed.length;

		if (trimmed.startsWith('- id:')) {
			if (current) {
				if (currentCheck) current.health_checks.push(currentCheck);
				result.push(current);
			}
			current = {
				id: trimmed.replace('- id:', '').trim().replace(/"/g, ''),
				name: '',
				hostname: '',
				power_on: 'wol',
				power_off: 'ssh',
				health_checks: [],
				depends_on: [],
			};
			inHealthChecks = false;
			currentCheck = null;
		} else if (current && !inHealthChecks) {
			const match = trimmed.match(/^(\w+):\s*(.*)$/);
			if (match) {
				const [, key, val] = match;
				const cleanVal = val.replace(/^["']|["']$/g, '').trim();
				if (key === 'health_checks') {
					inHealthChecks = true;
				} else if (key === 'depends_on') {
					// handled below as array
				} else {
					(current as any)[key] = cleanVal || undefined;
				}
			}
			if (trimmed.startsWith('- ') && !trimmed.startsWith('- id:')) {
				const depVal = trimmed.replace('- ', '').replace(/"/g, '').trim();
				if (!current.depends_on) current.depends_on = [];
				current.depends_on.push(depVal);
			}
		} else if (current && inHealthChecks) {
			if (trimmed.startsWith('- type:')) {
				if (currentCheck) current.health_checks.push(currentCheck);
				currentCheck = { type: trimmed.replace('- type:', '').trim() };
			} else if (currentCheck && trimmed.match(/^\w+:/)) {
				const match = trimmed.match(/^(\w+):\s*(.*)$/);
				if (match) {
					const [, key, val] = match;
					const cleanVal = val.replace(/^["']|["']$/g, '').trim();
					if (key === 'port') {
						currentCheck.port = parseInt(cleanVal);
					} else {
						(currentCheck as any)[key] = cleanVal;
					}
				}
			} else if (!trimmed.startsWith('-') && !trimmed.startsWith('#') && trimmed.includes(':') && indent <= 4) {
				inHealthChecks = false;
				if (currentCheck) {
					current.health_checks.push(currentCheck);
					currentCheck = null;
				}
				const match = trimmed.match(/^(\w+):\s*(.*)$/);
				if (match) {
					const [, key, val] = match;
					const cleanVal = val.replace(/^["']|["']$/g, '').trim();
					(current as any)[key] = cleanVal || undefined;
				}
			}
		}
	}
	if (currentCheck && current) current.health_checks.push(currentCheck);
	if (current) result.push(current);

	return { servers: result };
}
