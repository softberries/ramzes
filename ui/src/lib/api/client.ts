/** An error answer from the API, or a failure to reach it (status 0). */
export class ApiError extends Error {
	constructor(
		readonly status: number,
		readonly code: string,
		message: string
	) {
		super(message);
		this.name = 'ApiError';
	}
}

let unauthorized: (() => void) | null = null;

/** Registers what happens when the session has ended (the app sends the user to sign in). */
export function onUnauthorized(handler: (() => void) | null) {
	unauthorized = handler;
}

type Method = 'GET' | 'POST' | 'PUT' | 'DELETE';

/** Calls the admin API. Throws an `ApiError` for any answer that is not a success. */
export async function api<T = unknown>(method: Method, path: string, body?: unknown): Promise<T> {
	// The server requires this header on cookie-authenticated requests that change state.
	const headers: Record<string, string> = { 'x-ramzes-request': '1' };
	if (body !== undefined) headers['content-type'] = 'application/json';

	let response: Response;
	try {
		response = await fetch(path, {
			method,
			headers,
			body: body === undefined ? undefined : JSON.stringify(body),
			credentials: 'same-origin'
		});
	} catch {
		throw new ApiError(0, 'unreachable', 'ramzes could not be reached. Check that it is running.');
	}

	if (response.status === 204) return undefined as T;
	const text = await response.text();
	let data: unknown;
	try {
		data = text ? JSON.parse(text) : undefined;
	} catch {
		data = undefined;
	}

	if (!response.ok) {
		if (response.status === 401 && !path.startsWith('/api/auth/')) unauthorized?.();
		const problem = data as { error?: string; message?: string } | undefined;
		throw new ApiError(
			response.status,
			problem?.error ?? 'error',
			problem?.message ?? `The server answered with status ${response.status}.`
		);
	}
	return data as T;
}

/** The message to show for anything thrown by `api`. */
export function messageOf(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}
