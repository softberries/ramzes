import type { DeliveryState } from './api/types';

/** The part of the browser's EventSource this module uses; tests supply their own. */
export interface EventSourceLike {
	/** 0 connecting, 1 open, 2 closed for good. */
	readonly readyState: number;
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
const CLOSED = 2;
/** Wait before opening a new connection; doubles after each failure up to the maximum. */
const RETRY_MS = 2000;
const RETRY_MAX_MS = 30_000;

/** The browser's EventSource, seen through the narrower interface this module needs. */
function openEventSource(url: string): EventSourceLike {
	return new EventSource(url) as unknown as EventSourceLike;
}

/**
 * Live changes from the server.
 *
 * The browser reconnects by itself after a dropped connection, but gives up for good when the
 * server (or a proxy in front of it) answers with an error. In that case this opens a new
 * connection itself, waiting longer after each failure.
 */
export class LiveStream {
	status = $state<'connecting' | 'live' | 'reconnecting'>('connecting');
	private source: EventSourceLike | null = null;
	private handlers: { [K in keyof StreamMessages]: Handler<K>[] } = { event: [], delivery: [], resync: [] };
	private opener: ((url: string) => EventSourceLike) | null = null;
	private retryTimer: ReturnType<typeof setTimeout> | undefined;
	private retryMs = RETRY_MS;

	connect(open: (url: string) => EventSourceLike = openEventSource) {
		if (this.source) return;
		this.opener = open;
		this.open();
	}

	private open() {
		if (!this.opener) return;
		const source = this.opener('/api/stream');
		this.source = source;
		source.onopen = () => {
			// Whatever happened while disconnected was not seen.
			if (this.status === 'reconnecting') this.emit('resync', {});
			this.status = 'live';
			this.retryMs = RETRY_MS;
		};
		source.onerror = () => {
			this.status = 'reconnecting';
			if (source.readyState !== CLOSED || this.source !== source) return;
			source.close();
			this.source = null;
			this.retryTimer = setTimeout(() => this.open(), this.retryMs);
			this.retryMs = Math.min(this.retryMs * 2, RETRY_MAX_MS);
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
		clearTimeout(this.retryTimer);
		this.opener = null;
		this.source?.close();
		this.source = null;
		this.status = 'connecting';
		this.retryMs = RETRY_MS;
	}

	private emit<K extends keyof StreamMessages>(type: K, message: StreamMessages[K]) {
		for (const handler of [...this.handlers[type]] as Handler<K>[]) handler(message);
	}
}

export const live = new LiveStream();
