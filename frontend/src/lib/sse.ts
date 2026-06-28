import type { ServerState } from './types';

export type SseCallback = {
	onFullState: (servers: ServerState[]) => void;
	onUpdate: (server: ServerState) => void;
	onConfigReloaded: (data: { server_id: string; message: string }) => void;
};

export function connectSSE(callbacks: SseCallback): EventSource {
	const es = new EventSource('/api/events');

	es.addEventListener('full_state', (e) => {
		const servers: ServerState[] = JSON.parse(e.data);
		callbacks.onFullState(servers);
	});

	es.addEventListener('update', (e) => {
		const server: ServerState = JSON.parse(e.data);
		callbacks.onUpdate(server);
	});

	es.addEventListener('config_reloaded', (e) => {
		const data = JSON.parse(e.data);
		callbacks.onConfigReloaded(data);
	});

	return es;
}
