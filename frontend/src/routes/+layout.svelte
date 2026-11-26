<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { VERSION } from '$lib/version';
	import { getStats } from '$lib/api';
	import type { Stats } from '$lib/types';
	let { children } = $props();

	let stats = $state<Stats | null>(null);

	async function refreshStats() {
		try {
			stats = await getStats();
		} catch {
			// keep last known values
		}
	}

	onMount(() => {
		refreshStats();
		const timer = setInterval(refreshStats, 60_000);
		return () => clearInterval(timer);
	});

	function formatBytes(n: number): string {
		if (n < 1024) return `${n} B`;
		const units = ['KB', 'MB', 'GB'];
		let v = n / 1024;
		let i = 0;
		while (v >= 1024 && i < units.length - 1) {
			v /= 1024;
			i++;
		}
		return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
	}

	function historyDays(oldest: string | null): string {
		if (!oldest) return '0';
		const days = (Date.now() - new Date(oldest).getTime()) / 86_400_000;
		return days < 1 ? '<1' : String(Math.floor(days));
	}
</script>

<nav>
	<div class="nav-inner">
		<a href="/" class="logo">
			<svg class="logo-icon" viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
				<rect width="32" height="32" rx="7" fill="#1a1d27"/>
				<rect x="4" y="5" width="24" height="7" rx="2" fill="#2a2d3a"/>
				<rect x="6" y="7.5" width="13" height="2" rx="0.75" fill="#0f1117"/>
				<circle cx="25.5" cy="8.5" r="2.2" fill="#22c55e"/>
				<circle cx="24.7" cy="7.7" r="0.7" fill="rgba(255,255,255,0.35)"/>
				<rect x="4" y="13" width="24" height="7" rx="2" fill="#2a2d3a"/>
				<rect x="6" y="15.5" width="13" height="2" rx="0.75" fill="#0f1117"/>
				<circle cx="25.5" cy="16.5" r="2.2" fill="#22c55e"/>
				<circle cx="24.7" cy="15.7" r="0.7" fill="rgba(255,255,255,0.35)"/>
				<rect x="4" y="21" width="24" height="7" rx="2" fill="#2a2d3a"/>
				<rect x="6" y="23.5" width="13" height="2" rx="0.75" fill="#0f1117"/>
				<circle cx="25.5" cy="24.5" r="2.2" fill="#3b82f6"/>
				<circle cx="24.7" cy="23.7" r="0.7" fill="rgba(255,255,255,0.35)"/>
			</svg>
			servmgr<span class="version">v{VERSION}</span>
		</a>
		{#if stats}
			<div class="db-stats" title={stats.last_purge ? `Last purge: ${new Date(stats.last_purge).toLocaleString()}` : 'No purge yet'}>
				<span>DB {formatBytes(stats.db_size_bytes)}</span>
				<span class="sep">·</span>
				<span>{historyDays(stats.history_oldest)}d history{#if stats.history_retention_days > 0}<span class="muted"> / {stats.history_retention_days}d kept</span>{/if}</span>
			</div>
		{/if}
		<div class="nav-links">
			<a href="/">Dashboard</a>
			<a href="/config">Config</a>
		</div>
	</div>
</nav>

<main>
	{@render children()}
</main>

<style>
	nav {
		background: var(--color-surface);
		border-bottom: 1px solid var(--color-border);
		padding: 0.75rem 1rem;
		position: sticky;
		top: 0;
		z-index: 100;
	}
	.nav-inner {
		max-width: 1200px;
		margin: 0 auto;
		display: flex;
		align-items: center;
		justify-content: space-between;
	}
	.logo {
		font-weight: 700;
		font-size: 1.25rem;
		color: var(--color-text);
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.logo:hover {
		text-decoration: none;
	}
	.logo-icon {
		width: 24px;
		height: 24px;
		flex-shrink: 0;
	}
	.version {
		font-size: 0.7rem;
		font-weight: 400;
		color: var(--color-text-muted);
	}
	.db-stats {
		margin-left: auto;
		margin-right: 1.5rem;
		font-size: 0.75rem;
		color: var(--color-text-muted);
		display: flex;
		gap: 0.4rem;
		white-space: nowrap;
	}
	.db-stats .muted {
		opacity: 0.7;
	}
	@media (max-width: 640px) {
		.db-stats {
			display: none;
		}
	}
	.nav-links {
		display: flex;
		gap: 1.5rem;
	}
	main {
		max-width: 1200px;
		margin: 0 auto;
		padding: 1.5rem 1rem;
	}
</style>
