<script lang="ts">
	import type { ServerState, HistoryEntry, PowerLogEntry } from '$lib/types';
	import { forcePowerOn, forcePowerOff, getHistory, getPowerLog } from '$lib/api';

	let { server, onPowerOn, onPowerOff }: {
		server: ServerState;
		onPowerOn: () => void;
		onPowerOff: () => void;
	} = $props();

	function statusBadge(s: typeof server): { label: string; color: string } {
		switch (s.status) {
			case 'on':          return { label: 'On',          color: 'var(--color-green)' };
			case 'off':         return { label: 'Off',         color: 'var(--color-text-muted)' };
			case 'turning_on':  return { label: 'Turning On',  color: 'var(--color-blue)' };
			case 'turning_off': return { label: 'Turning Off', color: 'var(--color-blue)' };
			case 'degraded':    return { label: 'Degraded',    color: 'var(--color-orange)' };
			default:            return { label: s.status,      color: 'var(--color-text-muted)' };
		}
	}

	let badge = $derived(statusBadge(server));
	let hasError = $derived(!!server.config_error);

	// History popup
	let showHistory = $state(false);
	let historyRange = $state<'2h' | 'day' | 'week'>('day');
	let historyEntries = $state<HistoryEntry[]>([]);
	let historyLoading = $state(false);

	async function openHistory() {
		showHistory = true;
		await loadHistory();
	}

	async function loadHistory() {
		historyLoading = true;
		const msMap = { '2h': 2 * 3600_000, 'day': 86400_000, 'week': 7 * 86400_000 };
		const from = new Date(Date.now() - msMap[historyRange]).toISOString();
		historyEntries = await getHistory(server.id, from);
		historyLoading = false;
	}

	async function toggleRange(r: '2h' | 'day' | 'week') {
		historyRange = r;
		await loadHistory();
	}

	// Power log popup
	let showLog = $state(false);
	let logEntries = $state<PowerLogEntry[]>([]);
	let logLoading = $state(false);

	async function openLog() {
		showLog = true;
		logLoading = true;
		logEntries = await getPowerLog(server.id);
		logLoading = false;
	}

	function fmtTime(ts: string) {
		return new Date(ts).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' });
	}

	function fmtTimeShort(ts: string) {
		return new Date(ts).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
	}

	function segmentColor(status: string): string {
		switch (status) {
			case 'on':  return 'var(--color-green)';
			case 'turning_on': case 'turning_off': return 'var(--color-blue)';
			case 'degraded': return 'var(--color-orange)';
			case 'off': return '#3a3a3a';
			// legacy history values
			case 'up': return 'var(--color-green)';
			case 'down': return '#3a3a3a';
			default: return 'var(--color-red)';
		}
	}

	function statusLabel(s: string): string {
		const map: Record<string, string> = {
			on: 'On', off: 'Off',
			turning_on: 'Turning On', turning_off: 'Turning Off',
			degraded: 'Degraded', up: 'On', down: 'Off'
		};
		return map[s] ?? s;
	}

	type Segment = { flex: number; color: string; status: string; time: string };

	function computeSegments(entries: HistoryEntry[]): Segment[] {
		if (entries.length === 0) return [];
		const now = Date.now();
		const first = new Date(entries[0].timestamp).getTime();
		const total = now - first;
		if (total === 0) return [];
		return entries.map((e, i) => {
			const from = new Date(e.timestamp).getTime();
			const to = i + 1 < entries.length ? new Date(entries[i + 1].timestamp).getTime() : now;
			return {
				flex: (to - from) / total,
				color: segmentColor(e.status),
				status: e.status,
				time: fmtTime(e.timestamp)
			};
		});
	}

	type CounterPoint = { x: number; y: number };

	function computeCounterPoints(entries: HistoryEntry[]): CounterPoint[] {
		if (entries.length === 0) return [];
		const now = Date.now();
		const first = new Date(entries[0].timestamp).getTime();
		const total = now - first;
		if (total === 0) return [];
		const pts: CounterPoint[] = [];
		for (let i = 0; i < entries.length; i++) {
			const t = (new Date(entries[i].timestamp).getTime() - first) / total * 100;
			pts.push({ x: t, y: entries[i].counter });
			if (i + 1 < entries.length) {
				const t2 = (new Date(entries[i + 1].timestamp).getTime() - first) / total * 100;
				pts.push({ x: t2, y: entries[i].counter });
			}
		}
		pts.push({ x: 100, y: entries[entries.length - 1].counter });
		return pts;
	}

	function pointsToPolyline(pts: CounterPoint[], maxY: number, h: number): string {
		if (pts.length === 0 || maxY === 0) return '';
		return pts.map(p => `${p.x},${h - (p.y / maxY) * h}`).join(' ');
	}

	let segments = $derived(computeSegments(historyEntries));
	let counterPoints = $derived(computeCounterPoints(historyEntries));
	let counterMax = $derived(Math.max(1, ...historyEntries.map(e => e.counter)));
	let rangeStartLabel = $derived(historyEntries.length > 0 ? fmtTimeShort(historyEntries[0].timestamp) : '');

	// Hover tooltips
	let tooltip = $state<{ x: number; y: number; label: string; time: string } | null>(null);
	let counterHover = $state<{ pct: number; cy: number } | null>(null);

	function showSegTooltip(e: MouseEvent, seg: Segment) {
		tooltip = { x: e.clientX, y: e.clientY, label: statusLabel(seg.status), time: seg.time };
	}

	function hideTooltip() {
		tooltip = null;
		counterHover = null;
	}

	function entryAtPct(pct: number): HistoryEntry {
		const now = Date.now();
		const first = new Date(historyEntries[0].timestamp).getTime();
		const total = now - first;
		const targetTime = first + pct * total;
		let best = historyEntries[0];
		for (const entry of historyEntries) {
			if (new Date(entry.timestamp).getTime() <= targetTime) best = entry;
		}
		return best;
	}

	function handleCounterMove(e: MouseEvent) {
		if (historyEntries.length === 0) return;
		const svg = e.currentTarget as SVGElement;
		const rect = svg.getBoundingClientRect();
		const pct = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
		const entry = entryAtPct(pct);
		const cy = 40 - (entry.counter / counterMax) * 40;
		counterHover = { pct: pct * 100, cy };
		tooltip = { x: e.clientX, y: e.clientY, label: `×${entry.counter}`, time: fmtTime(entry.timestamp) };
	}

	async function handleForceOn() { await forcePowerOn(server.id); }
	async function handleForceOff() { await forcePowerOff(server.id); }
</script>

<div class="card" class:has-error={hasError}>
	<div class="header">
		<div>
			<a href="/servers/{server.id}" class="name">{server.name}</a>
			<span class="hostname">{server.hostname}</span>
		</div>
		<div class="header-right">
			<button class="btn-log" onclick={openLog} title="Power command log">Log</button>
			<button class="badge-btn" style="background: {badge.color}" onclick={openHistory} title="Status history">
				{badge.label}
			</button>
		</div>
	</div>

	{#if server.config_error}
		<div class="error-banner">{server.config_error}</div>
	{/if}

	<div class="checks">
		{#each [...server.checks].sort((a, b) => { const la = a.label ?? (a.port ? `${a.type}:${a.port}` : a.type); const lb = b.label ?? (b.port ? `${b.type}:${b.port}` : b.type); return la.localeCompare(lb); }) as check}
			<div class="check">
				<span class="check-icon" style="color: {check.ok ? 'var(--color-green)' : 'var(--color-red)'}">
					{check.ok ? '●' : '○'}
				</span>
				<span class="check-type">{check.label ?? (check.port ? `${check.type}:${check.port}` : check.type)}</span>
			</div>
		{/each}
	</div>

	{#if server.depends_on.length > 0}
		<div class="deps">
			<span class="deps-label">depends on</span>
			{#each server.depends_on as dep}
				<span class="dep-tag">{dep}</span>
			{/each}
		</div>
	{/if}

	<div class="footer">
		<div class="counter-display" title="Reference counter">
			{server.counter}
		</div>
		<div class="actions">
			<div class="main-actions">
				<button class="btn-counter-up" onclick={onPowerOn} disabled={hasError}>+1</button>
				<button class="btn-counter-down" onclick={onPowerOff} disabled={hasError}>-1</button>
			</div>
			<div class="force-actions">
				<button class="btn-force" onclick={handleForceOn} disabled={hasError}>Force On</button>
				<button class="btn-force" onclick={handleForceOff} disabled={hasError}>Force Off</button>
			</div>
		</div>
	</div>

	{#if server.last_checked}
		<span class="timestamp">checked {(() => {
			const mins = Math.floor((Date.now() - new Date(server.last_checked).getTime()) / 60000);
			if (mins < 1) return 'just now';
			if (mins === 1) return '1 min ago';
			return `${mins} min ago`;
		})()}</span>
	{/if}
</div>

<!-- History popup -->
{#if showHistory}
	<div class="overlay" onclick={() => { showHistory = false; hideTooltip(); }} role="presentation">
		<div class="popup" onclick={(e) => e.stopPropagation()} role="dialog" aria-label="Status history">
			<div class="popup-header">
				<span class="popup-title">Status history — {server.name}</span>
				<div class="range-toggle">
					<button class:active={historyRange === '2h'}   onclick={() => toggleRange('2h')}>2h</button>
					<button class:active={historyRange === 'day'}  onclick={() => toggleRange('day')}>1 day</button>
					<button class:active={historyRange === 'week'} onclick={() => toggleRange('week')}>1 week</button>
				</div>
				<button class="close-btn" onclick={() => { showHistory = false; hideTooltip(); }}>✕</button>
			</div>
			<div class="popup-body">
				{#if historyLoading}
					<p class="muted">Loading…</p>
				{:else if segments.length === 0}
					<p class="muted">No history available.</p>
				{:else}
					<div class="history-graph">
						<div class="history-bar">
							{#each segments as seg}
								<div
									class="seg"
									style="flex: {seg.flex}; background: {seg.color}"
									onmousemove={(e) => showSegTooltip(e, seg)}
									onmouseleave={hideTooltip}
								></div>
							{/each}
						</div>
						<div class="bar-axis">
							<span>{rangeStartLabel}</span>
							<span>now</span>
						</div>
						<div class="bar-legend">
							<span class="legend-item"><span class="legend-dot" style="background: var(--color-green)"></span>on</span>
							<span class="legend-item"><span class="legend-dot" style="background: var(--color-blue)"></span>turning</span>
							<span class="legend-item"><span class="legend-dot" style="background: var(--color-orange)"></span>degraded</span>
							<span class="legend-item"><span class="legend-dot" style="background: #3a3a3a"></span>off</span>
						</div>

						<div class="counter-chart-label">Counter</div>
						<div class="counter-chart">
							<svg
								viewBox="0 0 100 40"
								preserveAspectRatio="none"
								class="counter-svg"
								onmousemove={handleCounterMove}
								onmouseleave={hideTooltip}
							>
								<polyline
									points={pointsToPolyline(counterPoints, counterMax, 40)}
									fill="none"
									stroke="var(--color-blue)"
									stroke-width="1.5"
									vector-effect="non-scaling-stroke"
								/>
								{#if counterHover}
									<line
										x1={counterHover.pct} y1="0"
										x2={counterHover.pct} y2="40"
										stroke="rgba(255,255,255,0.12)"
										stroke-width="0.8"
										vector-effect="non-scaling-stroke"
									/>
									<circle
										cx={counterHover.pct}
										cy={counterHover.cy}
										r="2.5"
										fill="var(--color-blue)"
										stroke="var(--color-bg)"
										stroke-width="1.5"
										vector-effect="non-scaling-stroke"
									/>
								{/if}
							</svg>
							<div class="counter-y-labels">
								<span>{counterMax}</span>
								<span>0</span>
							</div>
						</div>
					</div>
				{/if}
			</div>
		</div>
	</div>
{/if}

<!-- Power log popup -->
{#if showLog}
	<div class="overlay" onclick={() => showLog = false} role="presentation">
		<div class="popup" onclick={(e) => e.stopPropagation()} role="dialog" aria-label="Power command log">
			<div class="popup-header">
				<span class="popup-title">Command log — {server.name}</span>
				<button class="close-btn" onclick={() => showLog = false}>✕</button>
			</div>
			<div class="popup-body">
				{#if logLoading}
					<p class="muted">Loading…</p>
				{:else if logEntries.length === 0}
					<p class="muted">No commands recorded yet.</p>
				{:else}
					<div class="log-list">
						{#each logEntries as entry}
							<div class="log-entry" class:log-fail={!entry.success}>
								<div class="log-header">
									<span class="log-icon">{entry.success ? '✓' : '✗'}</span>
									<span class="log-cmd">{entry.command}</span>
									<span class="log-time">{fmtTime(entry.timestamp)}</span>
								</div>
								{#if entry.message}
									<pre class="log-output">{entry.message}</pre>
								{/if}
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</div>
	</div>
{/if}

<!-- Floating tooltip (rendered above all overlays) -->
{#if tooltip}
	<div
		class="tooltip"
		style="left: {tooltip.x + 14}px; top: {tooltip.y - 52}px"
	>
		<span class="tooltip-label">{tooltip.label}</span>
		<span class="tooltip-time">{tooltip.time}</span>
	</div>
{/if}

<style>
	.card {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius);
		padding: 1rem;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}
	.card.has-error {
		border-color: var(--color-orange);
	}
	.header {
		display: flex;
		justify-content: space-between;
		align-items: flex-start;
	}
	.header-right {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		flex-shrink: 0;
	}
	.name {
		font-weight: 600;
		font-size: 1.1rem;
		color: var(--color-text);
	}
	.hostname {
		display: block;
		color: var(--color-text-muted);
		font-size: 0.8rem;
	}
	.badge-btn {
		font-size: 0.75rem;
		font-weight: 600;
		padding: 0.2rem 0.5rem;
		border-radius: 4px;
		color: #000;
		border: none;
		cursor: pointer;
		line-height: 1.4;
	}
	.btn-log {
		font-size: 0.7rem;
		padding: 0.2rem 0.4rem;
		background: var(--color-border);
		color: var(--color-text-muted);
		border: none;
		border-radius: 4px;
		cursor: pointer;
	}
	.btn-log:hover {
		color: var(--color-text);
	}
	.error-banner {
		background: rgba(249, 115, 22, 0.1);
		border: 1px solid var(--color-orange);
		border-radius: 4px;
		padding: 0.5rem;
		font-size: 0.8rem;
		color: var(--color-orange);
	}
	.checks {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	.check {
		display: flex;
		align-items: center;
		gap: 0.25rem;
		font-size: 0.8rem;
	}
	.check-type {
		color: var(--color-text-muted);
	}
	.deps {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		flex-wrap: wrap;
	}
	.deps-label {
		font-size: 0.7rem;
		color: var(--color-text-muted);
	}
	.dep-tag {
		font-size: 0.7rem;
		background: rgba(59, 130, 246, 0.15);
		color: var(--color-blue);
		padding: 0.15rem 0.4rem;
		border-radius: 3px;
		font-weight: 500;
	}
	.footer {
		display: flex;
		align-items: center;
		gap: 1rem;
		margin-top: auto;
	}
	.counter-display {
		font-size: 1.75rem;
		font-weight: 700;
		min-width: 3rem;
		text-align: center;
		background: var(--color-bg);
		border: 1px solid var(--color-border);
		border-radius: var(--radius);
		padding: 0.25rem 0.75rem;
	}
	.actions {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
		flex: 1;
	}
	.main-actions {
		display: flex;
		gap: 0.5rem;
	}
	.main-actions button {
		flex: 1;
		padding: 0.6rem 1rem;
		font-size: 0.9rem;
	}
	.btn-counter-up {
		background: rgba(34, 197, 94, 0.12);
		color: var(--color-green);
	}
	.btn-counter-down {
		background: rgba(239, 68, 68, 0.12);
		color: var(--color-red);
	}
	.force-actions {
		display: flex;
		gap: 0.5rem;
	}
	.btn-force {
		flex: 1;
		background: var(--color-border);
		color: var(--color-text-muted);
		font-size: 0.7rem;
		padding: 0.25rem 0.5rem;
	}
	.timestamp {
		font-size: 0.7rem;
		color: var(--color-text-muted);
	}

	/* Popups */
	.overlay {
		position: fixed;
		inset: 0;
		background: rgba(0, 0, 0, 0.5);
		display: flex;
		align-items: center;
		justify-content: center;
		z-index: 100;
	}
	.popup {
		background: var(--color-surface);
		border: 1px solid var(--color-border);
		border-radius: var(--radius);
		width: min(520px, 92vw);
		max-height: 70vh;
		display: flex;
		flex-direction: column;
		overflow: hidden;
	}
	.popup-header {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.75rem 1rem;
		border-bottom: 1px solid var(--color-border);
		flex-shrink: 0;
	}
	.popup-title {
		font-weight: 600;
		font-size: 0.9rem;
		flex: 1;
	}
	.close-btn {
		background: none;
		border: none;
		color: var(--color-text-muted);
		cursor: pointer;
		font-size: 1rem;
		padding: 0.1rem 0.3rem;
	}
	.close-btn:hover {
		color: var(--color-text);
	}
	.range-toggle {
		display: flex;
		gap: 0.25rem;
	}
	.range-toggle button {
		font-size: 0.75rem;
		padding: 0.15rem 0.5rem;
		background: var(--color-border);
		color: var(--color-text-muted);
		border: none;
		border-radius: 3px;
		cursor: pointer;
	}
	.range-toggle button.active {
		background: var(--color-blue);
		color: #fff;
	}
	.popup-body {
		overflow-y: auto;
		padding: 1rem;
		flex: 1;
	}
	.muted {
		color: var(--color-text-muted);
		font-size: 0.85rem;
	}

	/* History graph */
	.history-graph {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}
	.history-bar {
		display: flex;
		height: 36px;
		border-radius: 4px;
		overflow: hidden;
		gap: 1px;
		background: var(--color-border);
	}
	.seg {
		min-width: 2px;
		cursor: crosshair;
		transition: filter 0.1s;
	}
	.seg:hover {
		filter: brightness(1.3);
	}
	.bar-axis {
		display: flex;
		justify-content: space-between;
		font-size: 0.7rem;
		color: var(--color-text-muted);
		font-variant-numeric: tabular-nums;
	}
	.bar-legend {
		display: flex;
		gap: 1rem;
		font-size: 0.72rem;
		color: var(--color-text-muted);
		margin-top: 0.25rem;
	}
	.legend-item {
		display: flex;
		align-items: center;
		gap: 0.3rem;
	}
	.legend-dot {
		width: 8px;
		height: 8px;
		border-radius: 2px;
		flex-shrink: 0;
	}

	.counter-chart-label {
		font-size: 0.7rem;
		color: var(--color-text-muted);
		margin-top: 0.5rem;
	}
	.counter-chart {
		display: flex;
		align-items: stretch;
		gap: 0.4rem;
		height: 56px;
	}
	.counter-svg {
		flex: 1;
		display: block;
		border: 1px solid var(--color-border);
		border-radius: 3px;
		background: var(--color-bg);
		cursor: crosshair;
	}
	.counter-y-labels {
		display: flex;
		flex-direction: column;
		justify-content: space-between;
		font-size: 0.65rem;
		color: var(--color-text-muted);
		font-variant-numeric: tabular-nums;
		padding: 1px 0;
	}

	/* Hover tooltip */
	.tooltip {
		position: fixed;
		z-index: 200;
		background: #0a0c12;
		border: 1px solid var(--color-border);
		border-radius: 5px;
		padding: 0.3rem 0.6rem;
		font-size: 0.72rem;
		pointer-events: none;
		white-space: nowrap;
		box-shadow: 0 4px 14px rgba(0, 0, 0, 0.6);
		display: flex;
		flex-direction: column;
		gap: 0.1rem;
	}
	.tooltip-label {
		font-weight: 600;
		color: var(--color-text);
	}
	.tooltip-time {
		color: var(--color-text-muted);
	}

	/* Power log */
	.log-list {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
		font-size: 0.8rem;
	}
	.log-entry {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
	}
	.log-header {
		display: flex;
		align-items: center;
		gap: 0.5rem;
	}
	.log-icon { font-size: 0.85rem; color: var(--color-green); }
	.log-entry.log-fail .log-icon { color: var(--color-red); }
	.log-cmd { font-weight: 600; color: var(--color-text); }
	.log-time { color: var(--color-text-muted); font-variant-numeric: tabular-nums; margin-left: auto; }
	.log-output {
		margin: 0;
		padding: 0.5rem 0.6rem;
		background: var(--color-bg);
		border: 1px solid var(--color-border);
		border-radius: 4px;
		font-size: 0.75rem;
		color: var(--color-text-muted);
		white-space: pre-wrap;
		word-break: break-all;
		font-family: monospace;
		max-height: 8rem;
		overflow-y: auto;
	}
</style>
