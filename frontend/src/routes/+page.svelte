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

	// The backend no longer requires a matching caller to decrement — every
	// +1/-1 just moves the counter, floored at 0. The caller string here is
	// purely for the audit/power log.
	let callCounter = 0;

	async function handlePowerOn(id: string) {
		callCounter++;
		await powerOn(id, `webui-${callCounter}`);
	}

	async function handlePowerOff(id: string) {
		callCounter++;
		await powerOff(id, `webui-${callCounter}`);
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
