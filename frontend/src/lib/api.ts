import type { ServerState, HistoryEntry, PowerLogEntry } from './types';

const BASE = '/api';

export async function getServers(): Promise<ServerState[]> {
	const res = await fetch(`${BASE}/servers`);
	return res.json();
}

export async function getServer(id: string): Promise<ServerState> {
	const res = await fetch(`${BASE}/servers/${id}`);
	return res.json();
}

export async function powerOn(id: string, caller: string): Promise<ServerState> {
	const res = await fetch(`${BASE}/servers/${id}/powerinc`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ caller })
	});
	return res.json();
}

export async function powerOff(id: string, caller: string): Promise<ServerState> {
	const res = await fetch(`${BASE}/servers/${id}/powerdec`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ caller })
	});
	return res.json();
}

export async function forcePowerOn(id: string): Promise<ServerState> {
	const res = await fetch(`${BASE}/servers/${id}/poweron`, { method: 'POST' });
	return res.json();
}

export async function forcePowerOff(id: string): Promise<ServerState> {
	const res = await fetch(`${BASE}/servers/${id}/poweroff`, { method: 'POST' });
	return res.json();
}

export async function setCounter(id: string, value: number): Promise<ServerState> {
	const res = await fetch(`${BASE}/servers/${id}/counter`, {
		method: 'PUT',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ value })
	});
	return res.json();
}

export async function getHistory(id: string, from?: string, to?: string): Promise<HistoryEntry[]> {
	const params = new URLSearchParams();
	if (from) params.set('from', from);
	if (to) params.set('to', to);
	const res = await fetch(`${BASE}/servers/${id}/history?${params}`);
	return res.json();
}

export async function getPowerLog(id: string): Promise<PowerLogEntry[]> {
	const res = await fetch(`${BASE}/servers/${id}/powerlog`);
	return res.json();
}

export async function getConfig(): Promise<string> {
	const res = await fetch(`${BASE}/config`);
	return res.text();
}

export async function putConfig(yaml: string): Promise<void> {
	const res = await fetch(`${BASE}/config`, {
		method: 'PUT',
		headers: { 'Content-Type': 'text/plain' },
		body: yaml
	});
	if (!res.ok) {
		const err = await res.json();
		throw new Error(err.error || 'Failed to save config');
	}
}
