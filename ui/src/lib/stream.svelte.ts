import type { DeliveryState } from './api/types';

/** The part of the browser's EventSource this module uses; tests supply their own. */
export interface EventSourceLike {
	onopen: (() => void) | null;
	onerror: (() => void) | null;
	addEventListener(type: string, handler: (e: { data: string }) => void): void;
	close(): void;
}

export interface StreamMessages {
	/** An event has been routed. */
	event: { event_id: string };
	/** A delivery attempt finished. */
	delivery: { event_ids: string[]; delivery_id: string; state: DeliveryState };
	/** Changes may have been missed; reload what is on screen. */
	resync: Record<string, never>;
}

type Handler<K extends keyof StreamMessages> = (message: StreamMessages[K]) => void;

const TYPES: (keyof StreamMessages)[] = ['event', 'delivery', 'resync'];

/** Live changes from the server. The browser reconnects by itself when the connection drops. */
export class LiveStream {
	status = $state<'connecting' | 'live' | 'reconnecting'>('connecting');
	private source: EventSourceLike | null = null;
	private handlers: { [K in keyof StreamMessages]: Handler<K>[] } = { event: [], delivery: [], resync: [] };

	connect(open: (url: string) => EventSourceLike = (url) => new EventSource(url)) {
		if (this.source) return;
		const source = open('/api/stream');
		this.source = source;
		source.onopen = () => {
			// Whatever happened while disconnected was not seen.
			if (this.status === 'reconnecting') this.emit('resync', {});
			this.status = 'live';
		};
		source.onerror = () => {
			this.status = 'reconnecting';
		};
		for (const type of TYPES) {
			source.addEventListener(type, (e) => this.emit(type, JSON.parse(e.data)));
		}
	}

	/** Subscribes to one kind of message. Returns the function that unsubscribes. */
	on<K extends keyof StreamMessages>(type: K, handler: Handler<K>): () => void {
		const list = this.handlers[type] as Handler<K>[];
		list.push(handler);
		return () => {
			const at = list.indexOf(handler);
			if (at >= 0) list.splice(at, 1);
		};
	}

	close() {
		this.source?.close();
		this.source = null;
		this.status = 'connecting';
	}

	private emit<K extends keyof StreamMessages>(type: K, message: StreamMessages[K]) {
		for (const handler of [...this.handlers[type]] as Handler<K>[]) handler(message);
	}
}

export const live = new LiveStream();
