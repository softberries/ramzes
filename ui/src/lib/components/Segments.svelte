<script lang="ts">
	import type { Snippet } from 'svelte';

	/**
	 * The links or buttons that choose between views of one list, drawn as one control. The chosen
	 * one carries `aria-current="page"` or `aria-pressed="true"`; a number inside a `.count` is
	 * drawn as a pill, in the colour given as `--tone`.
	 */
	let { children }: { children: Snippet } = $props();
</script>

<div class="segments">{@render children()}</div>

<style>
	.segments {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem;
		width: fit-content;
		max-width: 100%;
		padding: 0.25rem;
		border: 1px solid var(--line);
		border-radius: var(--radius-card);
		background: var(--surface);
	}

	.segments :global(:is(a, button)) {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		height: 2rem;
		padding: 0 0.8rem;
		border-radius: var(--radius-control);
		color: var(--muted);
		font-weight: 500;
		text-decoration: none;
		white-space: nowrap;
		cursor: pointer;
	}

	.segments :global(:is(a, button):hover) {
		color: var(--fg);
	}

	.segments :global(:is([aria-current='page'], [aria-pressed='true'])) {
		background: var(--raised);
		color: var(--fg);
	}

	.segments :global(.count) {
		--tone: var(--muted);
		padding: 0 0.45rem;
		border-radius: 999px;
		background: color-mix(in srgb, var(--tone) 16%, transparent);
		color: var(--tone);
		font-size: 0.86rem;
		font-variant-numeric: tabular-nums;
	}
</style>
