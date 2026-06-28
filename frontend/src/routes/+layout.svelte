<script lang="ts">
	import '../app.css';
	import { VERSION } from '$lib/version';
	let { children } = $props();
</script>

<nav>
	<div class="nav-inner">
		<a href="/" class="logo">servmgr<span class="version">v{VERSION}</span></a>
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
		align-items: baseline;
		gap: 0.4rem;
	}
	.logo:hover {
		text-decoration: none;
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
