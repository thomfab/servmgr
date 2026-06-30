<script lang="ts">
	import '../app.css';
	import { VERSION } from '$lib/version';
	let { children } = $props();
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
