<script lang="ts">
	interface Props {
		/** The values, oldest first. */
		values: number[];
		/** A CSS colour for the line; the fill under it is a tint of the same. */
		color?: string;
	}

	let { values, color = 'var(--accent)' }: Props = $props();

	const WIDTH = 100;
	const HEIGHT = 28;

	const points = $derived.by(() => {
		if (values.length < 2) return [];
		const top = Math.max(...values, 1);
		return values.map((value, index) => [
			(index / (values.length - 1)) * WIDTH,
			// A stroke's width is kept clear of both edges.
			HEIGHT - 1.5 - (value / top) * (HEIGHT - 3)
		]);
	});
	const line = $derived(points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' '));
</script>

<!-- A shape of how a number moved; the number itself stands beside it. -->
{#if points.length > 0}
	<svg viewBox="0 0 {WIDTH} {HEIGHT}" preserveAspectRatio="none" class="h-7 w-full" aria-hidden="true">
		<polygon points="0,{HEIGHT} {line} {WIDTH},{HEIGHT}" fill={color} opacity="0.14" />
		<polyline points={line} fill="none" stroke={color} stroke-width="1.5" vector-effect="non-scaling-stroke" stroke-linejoin="round" />
	</svg>
{/if}
