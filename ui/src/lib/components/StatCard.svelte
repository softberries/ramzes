<script lang="ts">
	import type { Snippet } from 'svelte';
	import Icon from './Icon.svelte';
	import Sparkline from './Sparkline.svelte';

	interface Props {
		label: string;
		/** The figure, drawn by the page so it can be a link and carry a test id. */
		children: Snippet;
		/** How the figure moved against the stretch before: direction, and by how much. */
		trend?: { direction: 'up' | 'down' | 'same'; change: string | null };
		/** What the comparison is with: "the 24 hours before". */
		against?: string;
		/** Whether more is better, worse, or neither; it colours the comparison. */
		more?: 'good' | 'bad' | 'neutral';
		/** A line under the figure when there is no comparison to make. */
		note?: string;
		/** How the figure moved over the period, oldest first. */
		spark?: number[];
		sparkColor?: string;
	}

	let { label, children, trend, against, more = 'neutral', note, spark, sparkColor }: Props = $props();

	const tone = $derived.by(() => {
		if (!trend || trend.direction === 'same' || more === 'neutral') return 'text-muted';
		return (trend.direction === 'up') === (more === 'good') ? 'text-ok' : 'text-bad';
	});
</script>

<div class="card flex flex-col gap-1 p-4">
	<div class="text-muted">{label}</div>
	<div class="figure">{@render children()}</div>
	<div class="mt-auto flex min-h-6 items-center gap-1.5 text-[0.93rem]">
		{#if trend?.change}
			<span class="flex items-center gap-0.5 font-medium {tone}">
				{#if trend.direction !== 'same'}<Icon name={trend.direction} size={13} />{/if}
				{trend.change}
			</span>
			<span class="text-faint truncate">vs {against}</span>
		{:else if note}
			<span class="text-faint truncate">{note}</span>
		{/if}
	</div>
	{#if spark}<Sparkline values={spark} color={sparkColor} />{/if}
</div>

<style>
	.figure {
		font-size: 1.9rem;
		font-weight: 600;
		line-height: 1.2;
		letter-spacing: -0.03em;
		font-variant-numeric: tabular-nums;
	}

	.figure :global(a) {
		color: inherit;
		text-decoration-color: color-mix(in srgb, currentColor 35%, transparent);
	}
</style>
