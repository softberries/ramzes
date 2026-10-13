import { afterEach, describe, expect, it, vi } from 'vitest';
import { LiveStream, type EventSourceLike } from './stream.svelte';

/** A stand-in for EventSource that the test drives by hand. */
class FakeSource implements EventSourceLike {
	onopen: (() => void) | null = null;
	onerror: (() => void) | null = null;
	closed = false;
	readyState = 0;
	private handlers = new Map<string, (e: { data: string }) => void>();

	addEventListener(type: string, handler: (e: { data: string }) => void) {
		this.handlers.set(type, handler);
	}

	close() {
		this.closed = true;
	}

	emit(type: string, data: unknown) {
		this.handlers.get(type)?.({ data: JSON.stringify(data) });
	}
}

function connected() {
	const source = new FakeSource();
	const stream = new LiveStream();
	const urls: string[] = [];
	stream.connect((url) => {
		urls.push(url);
		return source;
	});
	return { source, stream, urls };
}

describe('LiveStream', () => {
	it('connects to the stream endpoint and reports its status', () => {
		const { source, stream, urls } = connected();
		expect(urls).toEqual(['/api/stream']);
		expect(stream.status).toBe('connecting');
		source.onopen?.();
		expect(stream.status).toBe('live');
		source.onerror?.();
		expect(stream.status).toBe('reconnecting');
		source.onopen?.();
		expect(stream.status).toBe('live');
	});

	it('delivers parsed messages to subscribers of that type', () => {
		const { source, stream } = connected();
		const onEvent = vi.fn();
		const onDelivery = vi.fn();
		stream.on('event', onEvent);
		stream.on('delivery', onDelivery);
		source.emit('event', { event_id: 'e1' });
		source.emit('delivery', { event_ids: ['e1'], delivery_id: 'd1', state: 'delivered' });
		expect(onEvent).toHaveBeenCalledWith({ event_id: 'e1' });
		expect(onDelivery).toHaveBeenCalledWith({ event_ids: ['e1'], delivery_id: 'd1', state: 'delivered' });
	});

	it('delivers a change of a notification', () => {
		const { source, stream } = connected();
		const onNotification = vi.fn();
		stream.on('notification', onNotification);
		source.emit('notification', { notification_id: 'n1' });
		expect(onNotification).toHaveBeenCalledWith({ notification_id: 'n1' });
	});

	it('stops delivering after unsubscribe', () => {
		const { source, stream } = connected();
		const handler = vi.fn();
		const off = stream.on('event', handler);
		off();
		source.emit('event', { event_id: 'e1' });
		expect(handler).not.toHaveBeenCalled();
	});

	it('tells subscribers to reload after a reconnect or a resync', () => {
		const { source, stream } = connected();
		const onResync = vi.fn();
		stream.on('resync', onResync);
		source.onopen?.();
		expect(onResync).not.toHaveBeenCalled();
		source.onerror?.();
		source.onopen?.();
		expect(onResync).toHaveBeenCalledTimes(1);
		source.emit('resync', {});
		expect(onResync).toHaveBeenCalledTimes(2);
	});

	it('connects only once and closes the source', () => {
		const { source, stream, urls } = connected();
		stream.connect(() => new FakeSource());
		expect(urls).toHaveLength(1);
		stream.close();
		expect(source.closed).toBe(true);
		expect(stream.status).toBe('connecting');
	});

	it('opens a new connection when the browser has given up on the old one', () => {
		vi.useFakeTimers();
		const sources: FakeSource[] = [];
		const stream = new LiveStream();
		stream.connect(() => {
			const source = new FakeSource();
			sources.push(source);
			return source;
		});
		const onResync = vi.fn();
		stream.on('resync', onResync);
		sources[0].onopen?.();

		// A proxy answered 502: the browser closes the stream and will not retry by itself.
		sources[0].readyState = 2;
		sources[0].onerror?.();
		expect(stream.status).toBe('reconnecting');
		expect(sources).toHaveLength(1);

		vi.advanceTimersByTime(2000);
		expect(sources).toHaveLength(2);
		expect(sources[0].closed).toBe(true);

		// Still failing: the next try waits longer.
		sources[1].readyState = 2;
		sources[1].onerror?.();
		vi.advanceTimersByTime(2000);
		expect(sources).toHaveLength(2);
		vi.advanceTimersByTime(2000);
		expect(sources).toHaveLength(3);

		sources[2].onopen?.();
		expect(stream.status).toBe('live');
		expect(onResync).toHaveBeenCalledTimes(1);
	});

	it('does not reconnect by itself while the browser is still retrying, or after close', () => {
		vi.useFakeTimers();
		let opened = 0;
		const source = new FakeSource();
		const stream = new LiveStream();
		stream.connect(() => {
			opened += 1;
			return source;
		});
		source.onerror?.(); // readyState 0: the browser is reconnecting on its own
		vi.advanceTimersByTime(60_000);
		expect(opened).toBe(1);

		source.readyState = 2;
		source.onerror?.();
		stream.close();
		vi.advanceTimersByTime(60_000);
		expect(opened).toBe(1);
	});
});

afterEach(() => {
	vi.useRealTimers();
});
