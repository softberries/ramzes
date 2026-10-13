/** Shown in place of a source's key when it is not known: it is readable only right after it is made. */
export const KEY_PLACEHOLDER = '<the source key>';

/**
 * A source kind's setup instructions with `{url}`, `{origin}` and `{key}` filled in.
 * `path` is the source's own address on this instance, as in `/v1/in/abc`.
 */
export function setupText(setup: string, at: { origin: string; path: string; key?: string }): string {
	const values: Record<string, string> = {
		url: at.origin + at.path,
		origin: at.origin,
		key: at.key ?? KEY_PLACEHOLDER
	};
	// One pass, so a value that itself contains a placeholder is not filled in again.
	return setup.replace(/\{(url|origin|key)\}/g, (_, name: string) => values[name]).trim();
}

/** The headers kept with a recorded request, as name and value pairs. */
export function headerList(stored: string | null | undefined): [string, string][] {
	if (!stored) return [];
	let value: unknown;
	try {
		value = JSON.parse(stored);
	} catch {
		return [];
	}
	if (typeof value !== 'object' || value === null || Array.isArray(value)) return [];
	return Object.entries(value).map(([name, text]) => [name, String(text)]);
}

/**
 * Whether a source is the one ramzes's own events come from, such as a connection being paused.
 * Nothing outside sends to it, so it has no address, no key to hand out and no requests.
 */
export function isBuiltIn(source: { kind: string }): boolean {
	return source.kind === 'system';
}
