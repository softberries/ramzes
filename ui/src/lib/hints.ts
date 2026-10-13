const PREFIX = 'ramzes-hint-';

/** Whether a hint was dismissed in this browser. A browser that refuses storage shows it every time. */
export function isDismissed(storage: Pick<Storage, 'getItem'>, hint: string): boolean {
	try {
		return storage.getItem(PREFIX + hint) !== null;
	} catch {
		return false;
	}
}

/** Remembers that a hint was dismissed in this browser. */
export function dismiss(storage: Pick<Storage, 'setItem'>, hint: string): void {
	try {
		storage.setItem(PREFIX + hint, '1');
	} catch {
		// Without storage the hint is back after a reload; it is still gone for now.
	}
}

/** Whether the instance has no address of its own. False while the settings are not loaded. */
export function lacksPublicUrl(settings: { public_url?: string | null } | null): boolean {
	return settings !== null && !settings.public_url?.trim();
}
