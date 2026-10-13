import { tick } from 'svelte';

/**
 * Brings the element with this id into view once the page has been updated. A form opens below
 * the list it belongs to, which can be far from the button that opened it.
 */
export async function reveal(id: string): Promise<void> {
	await tick();
	document.getElementById(id)?.scrollIntoView({ block: 'start' });
}
