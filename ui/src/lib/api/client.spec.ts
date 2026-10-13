import { afterEach, describe, expect, it, vi } from 'vitest';
import { ApiError, api, onUnauthorized } from './client';

function respond(status: number, body?: unknown) {
	const fetchMock = vi.fn(
		async () => new Response(body === undefined ? null : JSON.stringify(body), { status })
	);
	vi.stubGlobal('fetch', fetchMock);
	return fetchMock;
}

/** The error a call is expected to fail with. */
async function failure(call: Promise<unknown>): Promise<ApiError> {
	try {
		await call;
	} catch (error) {
		return error as ApiError;
	}
	throw new Error('the call was expected to fail');
}

afterEach(() => {
	vi.unstubAllGlobals();
	onUnauthorized(null);
});

describe('api', () => {
	it('sends the request header and a JSON body', async () => {
		const fetchMock = respond(200, { id: 'a' });
		const result = await api<{ id: string }>('POST', '/api/sources', { name: 'orders' });
		expect(result).toEqual({ id: 'a' });
		const [path, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
		expect(path).toBe('/api/sources');
		expect(init.method).toBe('POST');
		const headers = init.headers as Record<string, string>;
		expect(headers['x-ramzes-request']).toBe('1');
		expect(headers['content-type']).toBe('application/json');
		expect(init.body).toBe('{"name":"orders"}');
	});

	it('sends no body or content type for a GET', async () => {
		const fetchMock = respond(200, []);
		await api('GET', '/api/sources');
		const init = (fetchMock.mock.calls[0] as unknown as [string, RequestInit])[1];
		expect(init.body).toBeUndefined();
		expect((init.headers as Record<string, string>)['content-type']).toBeUndefined();
	});

	it('returns undefined for 204', async () => {
		respond(204);
		expect(await api('DELETE', '/api/sources/a')).toBeUndefined();
	});

	it('turns an error body into an ApiError', async () => {
		respond(422, { error: 'invalid', message: 'the name must be 1 to 100 characters' });
		const error = await failure(api('POST', '/api/sources', { name: '' }));
		expect(error).toBeInstanceOf(ApiError);
		expect(error.status).toBe(422);
		expect(error.code).toBe('invalid');
		expect(error.message).toBe('the name must be 1 to 100 characters');
	});

	it('explains an error that has no JSON body', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn(async () => new Response('<html>bad gateway</html>', { status: 502 }))
		);
		const error = await failure(api('GET', '/api/sources'));
		expect(error.status).toBe(502);
		expect(error.message).toBe('The server answered with status 502.');
	});

	it('explains a network failure', async () => {
		vi.stubGlobal(
			'fetch',
			vi.fn(async () => {
				throw new TypeError('Failed to fetch');
			})
		);
		const error = await failure(api('GET', '/api/sources'));
		expect(error).toBeInstanceOf(ApiError);
		expect(error.status).toBe(0);
		expect(error.message).toBe('ramzes could not be reached. Check that it is running.');
	});

	it('reports 401 to the unauthorized handler, except while signing in', async () => {
		const handler = vi.fn();
		onUnauthorized(handler);
		respond(401, { error: 'unauthorized', message: 'sign in or send an API token' });
		await api('GET', '/api/sources').catch(() => {});
		expect(handler).toHaveBeenCalledTimes(1);
		await api('POST', '/api/auth/login', {}).catch(() => {});
		await api('GET', '/api/auth/me').catch(() => {});
		expect(handler).toHaveBeenCalledTimes(1);
	});
});
