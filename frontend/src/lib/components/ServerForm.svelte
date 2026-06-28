<script lang="ts">
	interface HealthCheck {
		type: string;
		url?: string;
		port?: number;
	}

	interface ServerFormData {
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
		health_checks: HealthCheck[];
		check_interval_secs: number;
		power_on_timeout_secs: number;
	}

	let { initial, serverIds, onSave, onCancel }: {
		initial: ServerFormData | null;
		serverIds: string[];
		onSave: (data: ServerFormData) => void;
		onCancel: () => void;
	} = $props();

	let form = $state<ServerFormData>(initial ?? {
		id: '',
		name: '',
		hostname: '',
		power_on: 'wol',
		mac: '',
		wol_broadcast: '',
		power_off: 'ssh',
		ssh_user: '',
		ssh_key_path: '',
		ssh_password: '',
		ssh_shutdown_cmd: '',
		ipmi_ip: '',
		ipmi_user: '',
		ipmi_password: '',
		depends_on: '',
		health_checks: [{ type: 'ping' }],
		check_interval_secs: 30,
		power_on_timeout_secs: 300,
	});

	let isEdit = $derived(!!initial);
	let sshAuthMode = $state<'key' | 'password'>(initial?.ssh_password ? 'password' : 'key');

	function addCheck() {
		form.health_checks = [...form.health_checks, { type: 'ping' }];
	}

	function removeCheck(index: number) {
		form.health_checks = form.health_checks.filter((_, i) => i !== index);
	}

	function handleSubmit(e: Event) {
		e.preventDefault();
		onSave(form);
	}
</script>

<form class="server-form" onsubmit={handleSubmit}>
	<h2>{isEdit ? `Edit ${form.name}` : 'Add Server'}</h2>

	<fieldset>
		<legend>Basic Info</legend>
		<div class="field">
			<label for="id">ID</label>
			<input id="id" type="text" bind:value={form.id} placeholder="my-server" required disabled={isEdit} />
			<span class="hint">Unique identifier, used in URLs and dependencies</span>
		</div>
		<div class="field">
			<label for="name">Display Name</label>
			<input id="name" type="text" bind:value={form.name} placeholder="My Server" required />
		</div>
		<div class="field">
			<label for="hostname">Hostname / IP</label>
			<input id="hostname" type="text" bind:value={form.hostname} placeholder="192.168.1.100 or server.local" required />
		</div>
	</fieldset>

	<fieldset>
		<legend>Power On</legend>
		<div class="field">
			<label for="power_on">Method</label>
			<select id="power_on" bind:value={form.power_on}>
				<option value="wol">Wake-on-LAN</option>
				<option value="ipmi">IPMI</option>
			</select>
		</div>
		{#if form.power_on === 'wol'}
			<div class="field">
				<label for="mac">MAC Address</label>
				<input id="mac" type="text" bind:value={form.mac} placeholder="aa:bb:cc:dd:ee:ff" />
			</div>
			<div class="field">
				<label for="wol_broadcast">Subnet Broadcast</label>
				<input id="wol_broadcast" type="text" bind:value={form.wol_broadcast} placeholder="192.168.1.255" />
				<span class="hint">Optional — recommended when running in a VM (ESXi, Proxmox). Packet is sent to both this address and 255.255.255.255.</span>
			</div>
		{/if}
		{#if form.power_on === 'ipmi'}
			<div class="field">
				<label for="ipmi_ip">IPMI IP</label>
				<input id="ipmi_ip" type="text" bind:value={form.ipmi_ip} placeholder="192.168.1.201" />
			</div>
			<div class="field">
				<label for="ipmi_user">IPMI User</label>
				<input id="ipmi_user" type="text" bind:value={form.ipmi_user} placeholder="admin" />
			</div>
			<div class="field">
				<label for="ipmi_password">IPMI Password</label>
				<input id="ipmi_password" type="password" bind:value={form.ipmi_password} />
			</div>
		{/if}
	</fieldset>

	<fieldset>
		<legend>Power Off</legend>
		<div class="field">
			<label for="power_off">Method</label>
			<select id="power_off" bind:value={form.power_off}>
				<option value="ssh">SSH</option>
				<option value="ipmi">IPMI</option>
			</select>
		</div>
		{#if form.power_off === 'ssh'}
			<div class="field">
				<label for="ssh_user">SSH User</label>
				<input id="ssh_user" type="text" bind:value={form.ssh_user} placeholder="root" />
			</div>
			<div class="field">
				<label>Authentication</label>
				<div class="radio-group">
					<label class="radio-label">
						<input type="radio" name="ssh_auth" value="key" checked={sshAuthMode === 'key'} onchange={() => { sshAuthMode = 'key'; form.ssh_password = ''; }} />
						Key file
					</label>
					<label class="radio-label">
						<input type="radio" name="ssh_auth" value="password" checked={sshAuthMode === 'password'} onchange={() => { sshAuthMode = 'password'; form.ssh_key_path = ''; }} />
						Password
					</label>
				</div>
			</div>
			{#if sshAuthMode === 'key'}
				<div class="field">
					<label for="ssh_key_path">SSH Key Path</label>
					<input id="ssh_key_path" type="text" bind:value={form.ssh_key_path} placeholder="/config/id_rsa" />
					<span class="hint">Path inside the container</span>
				</div>
			{:else}
				<div class="field">
					<label for="ssh_password">SSH Password</label>
					<input id="ssh_password" type="password" bind:value={form.ssh_password} />
				</div>
			{/if}
			<div class="field">
				<label for="ssh_shutdown_cmd">Shutdown Command</label>
				<input id="ssh_shutdown_cmd" type="text" bind:value={form.ssh_shutdown_cmd} placeholder="sudo shutdown -h now" />
				<span class="hint">Leave empty for default: sudo shutdown -h now</span>
			</div>
		{/if}
		{#if form.power_off === 'ipmi' && form.power_on !== 'ipmi'}
			<div class="field">
				<label for="ipmi_ip2">IPMI IP</label>
				<input id="ipmi_ip2" type="text" bind:value={form.ipmi_ip} placeholder="192.168.1.201" />
			</div>
			<div class="field">
				<label for="ipmi_user2">IPMI User</label>
				<input id="ipmi_user2" type="text" bind:value={form.ipmi_user} placeholder="admin" />
			</div>
			<div class="field">
				<label for="ipmi_password2">IPMI Password</label>
				<input id="ipmi_password2" type="password" bind:value={form.ipmi_password} />
			</div>
		{/if}
	</fieldset>

	<fieldset>
		<legend>Health Checks</legend>
		{#each form.health_checks as check, i}
			<div class="check-row">
				<select bind:value={check.type}>
					<option value="ping">Ping</option>
					<option value="http">HTTP</option>
					<option value="tcp">TCP Port</option>
					<option value="ssh">SSH (port 22)</option>
					<option value="ipmi_power">IPMI Power Status</option>
				</select>
				{#if check.type === 'http'}
					<input type="text" bind:value={check.url} placeholder="http://host:port/path" class="check-input" />
				{/if}
				{#if check.type === 'tcp'}
					<input type="number" bind:value={check.port} placeholder="Port" min="1" max="65535" class="check-input-sm" />
				{/if}
				<button type="button" class="btn-remove" onclick={() => removeCheck(i)}>×</button>
			</div>
		{/each}
		<button type="button" class="btn-add" onclick={addCheck}>+ Add Check</button>
	</fieldset>

	<fieldset>
		<legend>Dependencies</legend>
		{#if serverIds.filter(s => s !== form.id).length === 0}
			<span class="hint">No other servers configured yet.</span>
		{:else}
			<span class="hint">This server needs these servers to be running first:</span>
			<div class="dep-checkboxes">
				{#each serverIds.filter(s => s !== form.id) as depId}
					<label class="dep-check">
						<input
							type="checkbox"
							checked={form.depends_on.split(',').map(s => s.trim()).includes(depId)}
							onchange={(e: Event) => {
								const target = e.target as HTMLInputElement;
								let deps = form.depends_on.split(',').map(s => s.trim()).filter(Boolean);
								if (target.checked) {
									deps.push(depId);
								} else {
									deps = deps.filter(d => d !== depId);
								}
								form.depends_on = deps.join(', ');
							}}
						/>
						{depId}
					</label>
				{/each}
			</div>
		{/if}
	</fieldset>

	<fieldset>
		<legend>Timing</legend>
		<div class="field-row">
			<div class="field">
				<label for="check_interval">Check Interval (seconds)</label>
				<input id="check_interval" type="number" bind:value={form.check_interval_secs} min="5" max="3600" />
			</div>
			<div class="field">
				<label for="power_timeout">Power On Timeout (seconds)</label>
				<input id="power_timeout" type="number" bind:value={form.power_on_timeout_secs} min="30" max="3600" />
			</div>
		</div>
	</fieldset>

	<div class="form-actions">
		<button type="button" class="btn-cancel" onclick={onCancel}>Cancel</button>
		<button type="submit" class="btn-blue">{isEdit ? 'Save Changes' : 'Add Server'}</button>
	</div>
</form>

<style>
	.server-form {
		display: flex;
		flex-direction: column;
		gap: 1.25rem;
	}
	h2 {
		font-size: 1.25rem;
		margin-bottom: 0.5rem;
	}
	fieldset {
		border: 1px solid var(--color-border);
		border-radius: var(--radius);
		padding: 1rem;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}
	legend {
		font-weight: 600;
		font-size: 0.85rem;
		color: var(--color-text-muted);
		padding: 0 0.5rem;
	}
	.field {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.field-row {
		display: flex;
		gap: 1rem;
	}
	.field-row .field {
		flex: 1;
	}
	label {
		font-size: 0.8rem;
		font-weight: 500;
		color: var(--color-text-muted);
	}
	input, select {
		background: var(--color-bg);
		color: var(--color-text);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 0.5rem 0.75rem;
		font-size: 0.875rem;
	}
	input:focus, select:focus {
		outline: 1px solid var(--color-blue);
		border-color: var(--color-blue);
	}
	input:disabled {
		opacity: 0.5;
	}
	.hint {
		font-size: 0.7rem;
		color: var(--color-text-muted);
	}
	.check-row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}
	.check-row select {
		min-width: 140px;
	}
	.check-input {
		flex: 1;
	}
	.check-input-sm {
		width: 100px;
	}
	.btn-remove {
		background: var(--color-red);
		color: #fff;
		width: 28px;
		height: 28px;
		padding: 0;
		font-size: 1.1rem;
		line-height: 1;
		border-radius: 4px;
	}
	.btn-add {
		background: var(--color-border);
		color: var(--color-text);
		font-size: 0.8rem;
		padding: 0.4rem 0.75rem;
		align-self: flex-start;
	}
	.form-actions {
		display: flex;
		justify-content: flex-end;
		gap: 0.75rem;
		padding-top: 0.5rem;
	}
	.btn-cancel {
		background: var(--color-border);
		color: var(--color-text);
	}
	.radio-group {
		display: flex;
		gap: 1.5rem;
	}
	.radio-label {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.85rem;
		cursor: pointer;
	}
	.radio-label input[type="radio"] {
		width: auto;
		padding: 0;
	}
	.dep-checkboxes {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
	}
	.dep-check {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		font-size: 0.85rem;
		cursor: pointer;
		background: var(--color-bg);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		padding: 0.35rem 0.65rem;
	}
	.dep-check input[type="checkbox"] {
		width: auto;
		padding: 0;
	}
</style>
