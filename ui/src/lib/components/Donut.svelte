<script lang="ts">
	interface Props {
		/** The parts of the whole, in the order they are drawn, clockwise from the top. */
		parts: { label: string; value: number; color: string }[];
		/** What stands in the middle: the figure, and what it is. */
		figure: string;
		caption: string;
	}

	let { parts, figure, caption }: Props = $props();

	const RADIUS = 42;
	const AROUND = 2 * Math.PI * RADIUS;
	/** The gap between two parts, along the ring. */
	const GAP = 2.5;

	const whole = $derived(parts.reduce((sum, part) => sum + part.value, 0));
	const arcs = $derived.by(() => {
		const drawn = parts.filter((part) => part.value > 0);
		let before = 0;
		return drawn.map((part) => {
			const length = (part.value / whole) * AROUND;
			const arc = { ...part, length: Math.max(length - (drawn.length > 1 ? GAP : 0), 0.5), offset: -before };
			before += length;
			return arc;
		});
	});
</script>

<!-- The numbers are given as text beside the ring by whoever uses it. -->
<svg viewBox="0 0 100 100" class="h-36 w-36 shrink-0" aria-hidden="true">
	<circle cx="50" cy="50" r={RADIUS} fill="none" stroke="var(--raised)" stroke-width="9" />
	{#each arcs as arc (arc.label)}
		<circle
			cx="50"
			cy="50"
			r={RADIUS}
			fill="none"
			stroke={arc.color}
			stroke-width="9"
			stroke-dasharray="{arc.length} {AROUND}"
			stroke-dashoffset={arc.offset}
			transform="rotate(-90 50 50)"
		/>
	{/each}
	<text x="50" y="49" text-anchor="middle" class="figure">{figure}</text>
	<text x="50" y="63" text-anchor="middle" class="caption">{caption}</text>
</svg>

<style>
	.figure {
		fill: var(--fg);
		font-size: 17px;
		font-weight: 600;
		letter-spacing: -0.02em;
	}

	.caption {
		fill: var(--muted);
		font-size: 7.5px;
	}
</style>
