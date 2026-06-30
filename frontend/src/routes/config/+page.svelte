<script lang="ts">
	import { onMount } from 'svelte';
	import { getConfig, putConfig } from '$lib/api';
	import ServerForm from '$lib/components/ServerForm.svelte';
	import { serversToYaml, parseConfigYaml } from '$lib/config-yaml';

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
		health_checks: { type: string; url?: string; port?: number; label?: string }[];
		check_interval_secs?: number;
		power_on_timeout_secs?: number;
	}

	interface FormData {
		id: string;
		name: string;
		hostname: string;
		power_on: string;
		mac: string;
		wol_broadcast: string;
		power_off: string;
		ssh_user: string;
		ssh_key_path: string;
		ssh_password: string;
		ssh_shutdown_cmd: string;
		ipmi_ip: string;
		ipmi_user: string;
		ipmi_password: string;
		depends_on: string;
		health_checks: { type: string; url?: string; port?: number; label?: string }[];
		check_interval_secs: number;
		power_on_timeout_secs: number;
	}

	let servers = $state<ServerEntry[]>([]);
	let editing = $state<FormData | null>(null);
	let adding = $state(false);
	let message = $state<{ type: 'success' | 'error'; text: string } | null>(null);

	let serverIds = $derived(servers.map(s => s.id));

	onMount(async () => {
		await loadConfig();
	});

	async function loadConfig() {
		const yaml = await getConfig();
		try {
			const parsed = parseConfigYaml(yaml);
			servers = parsed.servers || [];
		} catch {
			servers = [];
		}
	}

	function entryToForm(entry: ServerEntry): FormData {
		return {
			id: entry.id,
			name: entry.name,
			hostname: entry.hostname,
			power_on: entry.power_on,
			mac: entry.mac || '',
			wol_broadcast: entry.wol_broadcast || '',
			power_off: entry.power_off,
			ssh_user: entry.ssh_user || '',
			ssh_key_path: entry.ssh_key_path || '',
			ssh_password: entry.ssh_password || '',
			ssh_shutdown_cmd: entry.ssh_shutdown_cmd || '',
			ipmi_ip: entry.ipmi_ip || '',
			ipmi_user: entry.ipmi_user || '',
			ipmi_password: entry.ipmi_password || '',
			depends_on: (entry.depends_on || []).join(', '),
			health_checks: entry.health_checks.length > 0 ? entry.health_checks : [{ type: 'ping' }],
			check_interval_secs: entry.check_interval_secs ? Number(entry.check_interval_secs) : 30,
			power_on_timeout_secs: entry.power_on_timeout_secs ? Number(entry.power_on_timeout_secs) : 300,
		};
	}

	function formToEntry(form: FormData): ServerEntry {
		const entry: ServerEntry = {
			id: form.id,
			name: form.name,
			hostname: form.hostname,
			power_on: form.power_on,
			power_off: form.power_off,
			health_checks: form.health_checks,
			check_interval_secs: form.check_interval_secs,
			power_on_timeout_secs: form.power_on_timeout_secs,
		};
		if (form.mac) entry.mac = form.mac;
		if (form.wol_broadcast) entry.wol_broadcast = form.wol_broadcast;
		if (form.ssh_user) entry.ssh_user = form.ssh_user;
		if (form.ssh_key_path) entry.ssh_key_path = form.ssh_key_path;
		if (form.ssh_password) entry.ssh_password = form.ssh_password;
		if (form.ssh_shutdown_cmd) entry.ssh_shutdown_cmd = form.ssh_shutdown_cmd;
		if (form.ipmi_ip) entry.ipmi_ip = form.ipmi_ip;
		if (form.ipmi_user) entry.ipmi_user = form.ipmi_user;
		if (form.ipmi_password) entry.ipmi_password = form.ipmi_password;
		const deps = form.depends_on.split(',').map(s => s.trim()).filter(Boolean);
		if (deps.length > 0) entry.depends_on = deps;
		return entry;
	}

	async function saveAll(newServers: ServerEntry[]) {
		message = null;
		const yaml = serversToYaml(newServers);
		try {
			await putConfig(yaml);
			servers = newServers;
			message = { type: 'success', text: 'Config saved.' };
			editing = null;
			adding = false;
		} catch (e: any) {
			message = { type: 'error', text: e.message || 'Failed to save.' };
		}
	}

	function startEdit(entry: ServerEntry) {
		adding = false;
		editing = entryToForm(entry);
	}

	function startAdd() {
		editing = null;
		adding = true;
	}

	function handleSave(form: FormData) {
		const entry = formToEntry(form);
		let newServers: ServerEntry[];
		if (adding) {
			newServers = [...servers, entry];
		} else {
			newServers = servers.map(s => s.id === entry.id ? entry : s);
		}
		saveAll(newServers);
	}

	function handleRemove(id: string) {
		if (!confirm(`Remove server "${id}"?`)) return;
		const newServers = servers.filter(s => s.id !== id);
		saveAll(newServers);
	}

	function handleCancel() {
		editing = null;
		adding = false;
	}
</script>

<svelte:head>
	<title>servmgr - Config</title>
</svelte:head>

<div class="config-page">
	{#if editing || adding}
		<ServerForm
			initial={editing}
			{serverIds}
			onSave={handleSave}
			onCancel={handleCancel}
		/>
	{:else}
		<header>
			<h1>Servers</h1>
			<button class="btn-blue" onclick={startAdd}>+ Add Server</button>
		</header>

		{#if message}
			<div class="message {message.type}">{message.text}</div>
		{/if}

		{#if servers.length === 0}
			<div class="empty">
				<p>No servers configured yet.</p>
				<p>Click "Add Server" to get started.</p>
			</div>
		{:else}
			<div class="server-list">
				{#each [...servers].sort((a, b) => a.id.localeCompare(b.id)) as server (server.id)}
					<div class="server-row">
						<div class="server-info">
							<span class="server-name">{server.name}</span>
							<span class="server-meta">{server.hostname} · {server.power_on}/{server.power_off} · {server.health_checks.length} check{server.health_checks.length !== 1 ? 's' : ''}</span>
							{#if server.depends_on && server.depends_on.length > 0}
								<span class="server-deps">depends on: {server.depends_on.join(', ')}</span>
							{/if}
						</div>
						<div class="server-actions">
							<button class="btn-edit" onclick={() => startEdit(server)}>Edit</button>
							<button class="btn-remove" onclick={() => handleRemove(server.id)}>Remove</button>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	{/if}
</div>

<style>
	.config-page {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	header {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}
	h1 {
		font-size: 1.25rem;
	}
	.message {
		padding: 0.5rem 0.75rem;
		border-radius: var(--radius);
		font-size: 0.875rem;
	}
	.message.success {
		background: rgba(34, 197, 94, 0.1);
		border: 1px solid var(--color-green);
		color: var(--color-green);
	}
	.message.error {
		background: rgba(239, 68, 68, 0.1);
		border: 1px solid var(--color-red);
		color: var(--color-red);
	}
	.empty {
		text-align: center;
		color: var(--color-text-muted);
		padding: 3rem;
	}
	.server-list {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	.server-row {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius);
		padding: 1rem;
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
	}
	.server-info {
		display: flex;
		flex-direction: column;
		gap: 0.15rem;
	}
	.server-name {
		font-weight: 600;
		font-size: 1rem;
	}
	.server-meta {
		font-size: 0.8rem;
		color: var(--color-text-muted);
	}
	.server-deps {
		font-size: 0.75rem;
		color: var(--color-blue);
	}
	.server-actions {
		display: flex;
		gap: 0.5rem;
		flex-shrink: 0;
	}
	.btn-edit {
		background: var(--color-border);
		color: var(--color-text);
		font-size: 0.8rem;
		padding: 0.35rem 0.75rem;
	}
	.btn-remove {
		background: rgba(239, 68, 68, 0.15);
		color: var(--color-red);
		font-size: 0.8rem;
		padding: 0.35rem 0.75rem;
	}
</style>
