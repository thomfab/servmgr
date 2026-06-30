<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { page } from '$app/state';
	import type { ServerState, HistoryEntry } from '$lib/types';
	import { getServer, getHistory, powerOn, powerOff } from '$lib/api';
	import { connectSSE } from '$lib/sse';

	let server = $state<ServerState | null>(null);
	let history = $state<HistoryEntry[]>([]);
	let eventSource: EventSource | null = null;

	let id = $derived(page.params.id);

	onMount(async () => {
		server = await getServer(id);
		history = await getHistory(id);

		eventSource = connectSSE({
			onFullState(servers) {
				const s = servers.find(srv => srv.id === id);
				if (s) server = s;
			},
			onUpdate(s) {
				if (s.id === id) server = s;
			},
			onConfigReloaded() {}
		});
	});

	onDestroy(() => {
		eventSource?.close();
	});

	let callCounter = 0;

	async function handlePowerOn() {
		if (!server) return;
		callCounter++;
		await powerOn(server.id, `webui-${callCounter}`);
	}

	async function handlePowerOff() {
		if (!server) return;
		const webuiCallers = server.callers.filter(c => c.startsWith('webui-'));
		const caller = webuiCallers[webuiCallers.length - 1];
		if (caller) {
			await powerOff(server.id, caller);
		}
	}
</script>

<svelte:head>
	<title>servmgr - {server?.name ?? 'Server'}</title>
</svelte:head>

{#if server}
	<div class="detail">
		<header>
			<div>
				<h1>{server.name}</h1>
				<span class="hostname">{server.hostname}</span>
			</div>
			<div class="actions">
				<button class="btn-green" onclick={handlePowerOn} disabled={!!server.config_error}>+1</button>
				<button class="btn-red" onclick={handlePowerOff} disabled={!!server.config_error}>-1</button>
			</div>
		</header>

		{#if server.config_error}
			<div class="error-banner">{server.config_error}</div>
		{/if}

		<section>
			<h2>Status</h2>
			<div class="status-row">
				<span>Status: <strong>{server.status}</strong></span>
				<span>Counter: <strong>{server.counter}</strong></span>
			</div>
			{#if server.callers.length > 0}
				<div class="callers">
					Active callers: {server.callers.join(', ')}
				</div>
			{/if}
		</section>

		<section>
			<h2>Health Checks</h2>
			<div class="checks-table">
				{#each server.checks as check}
					<div class="check-row">
						<span class="indicator" class:ok={check.ok} class:fail={!check.ok}></span>
						<span class="check-name">{check.type}{check.port ? `:${check.port}` : ''}</span>
						<span class="latency">{check.latency_ms ? `${check.latency_ms}ms` : '-'}</span>
					</div>
				{/each}
			</div>
			{#if server.last_checked}
				<p class="last-checked">Last checked: {new Date(server.last_checked).toLocaleString()}</p>
			{/if}
		</section>

		<section>
			<h2>History (last 24h)</h2>
			{#if history.length === 0}
				<p class="muted">No history available.</p>
			{:else}
				<div class="history">
					{#each history.slice(-50) as entry}
						<div class="history-entry">
							<span class="time">{new Date(entry.timestamp).toLocaleTimeString()}</span>
							<span class="status-badge {entry.status}">{entry.status}</span>
							<span class="counter-badge">×{entry.counter}</span>
						</div>
					{/each}
				</div>
			{/if}
		</section>
	</div>
{:else}
	<p>Loading...</p>
{/if}

<style>
	.detail {
		display: flex;
		flex-direction: column;
		gap: 1.5rem;
	}
	header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
		flex-wrap: wrap;
		gap: 1rem;
	}
	h1 {
		font-size: 1.5rem;
	}
	.hostname {
		color: var(--color-text-muted);
		font-size: 0.9rem;
	}
	.actions {
		display: flex;
		gap: 0.5rem;
	}
	.error-banner {
		background: rgba(249, 115, 22, 0.1);
		border: 1px solid var(--color-orange);
		border-radius: var(--radius);
		padding: 0.75rem;
		color: var(--color-orange);
	}
	section {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius);
		padding: 1rem;
	}
	h2 {
		font-size: 1rem;
		margin-bottom: 0.75rem;
		color: var(--color-text-muted);
	}
	.status-row {
		display: flex;
		gap: 2rem;
		flex-wrap: wrap;
	}
	.callers {
		margin-top: 0.5rem;
		font-size: 0.85rem;
		color: var(--color-text-muted);
	}
	.checks-table {
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	.check-row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
	}
	.indicator {
		width: 8px;
		height: 8px;
		border-radius: 50%;
	}
	.indicator.ok {
		background: var(--color-green);
	}
	.indicator.fail {
		background: var(--color-red);
	}
	.check-name {
		flex: 1;
	}
	.latency {
		color: var(--color-text-muted);
		font-size: 0.85rem;
	}
	.last-checked {
		margin-top: 0.75rem;
		font-size: 0.8rem;
		color: var(--color-text-muted);
	}
	.muted {
		color: var(--color-text-muted);
	}
	.history {
		max-height: 300px;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.history-entry {
		display: flex;
		gap: 1rem;
		font-size: 0.85rem;
		align-items: center;
	}
	.time {
		color: var(--color-text-muted);
		min-width: 80px;
	}
	.status-badge {
		padding: 0.1rem 0.4rem;
		border-radius: 3px;
		font-size: 0.75rem;
		font-weight: 600;
	}
	.status-badge.on { background: var(--color-green); color: #000; }
	.status-badge.degraded { background: var(--color-orange); color: #000; }
	.status-badge.turning_on, .status-badge.turning_off { background: var(--color-blue); color: #fff; }
	.status-badge.off { background: var(--color-border); color: var(--color-text-muted); }
	.counter-badge {
		color: var(--color-text-muted);
		font-size: 0.75rem;
	}
</style>
