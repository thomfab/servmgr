<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import type { ServerState } from '$lib/types';
	import { connectSSE } from '$lib/sse';
	import { powerOn, powerOff } from '$lib/api';
	import ServerCard from '$lib/components/ServerCard.svelte';

	let servers = $state<ServerState[]>([]);
	let eventSource: EventSource | null = null;

	function sortById(arr: ServerState[]): ServerState[] {
		return [...arr].sort((a, b) => a.id.localeCompare(b.id));
	}

	onMount(() => {
		eventSource = connectSSE({
			onFullState(s) { servers = sortById(s); },
			onUpdate(s) {
				servers = servers.map(srv => srv.id === s.id ? s : srv);
			},
			onConfigReloaded() {
				// Full state will follow
			}
		});
	});

	onDestroy(() => {
		eventSource?.close();
	});

	let callCounter = 0;
	// Tracks the caller used for each server's last +1 in this session.
	// Allows -1 to work immediately after +1 without waiting for the SSE update.
	let sessionCallers: Record<string, string> = {};

	async function handlePowerOn(id: string) {
		callCounter++;
		const caller = `webui-${callCounter}`;
		sessionCallers[id] = caller;
		await powerOn(id, caller);
	}

	async function handlePowerOff(id: string) {
		const caller = sessionCallers[id]
			?? servers.find(s => s.id === id)?.callers.filter(c => c.startsWith('webui-')).at(-1);
		if (!caller) return;
		await powerOff(id, caller);
		delete sessionCallers[id];
	}
</script>

<svelte:head>
	<title>servmgr - Dashboard</title>
</svelte:head>

<div class="dashboard">
	{#if servers.length === 0}
		<p class="empty">No servers configured. <a href="/config">Add servers in the config editor.</a></p>
	{:else}
		<div class="grid">
			{#each servers as server (server.id)}
				<ServerCard
					{server}
					onPowerOn={() => handlePowerOn(server.id)}
					onPowerOff={() => handlePowerOff(server.id)}
				/>
			{/each}
		</div>
	{/if}
</div>

<style>
	.dashboard {
		width: 100%;
	}
	.empty {
		text-align: center;
		color: var(--color-text-muted);
		padding: 3rem 1rem;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
		gap: 1rem;
	}
	@media (max-width: 640px) {
		.grid {
			grid-template-columns: 1fr;
		}
	}
</style>
