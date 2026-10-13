<script lang="ts" module>
	/** Line drawings on a 24 by 24 grid: one path each, drawn with the text colour. */
	const PATHS = {
		overview: 'M4 20V4M4 20h16M8.5 16v-5M12.5 16V8M16.5 16v-3',
		feed: 'M4 6h16M4 12h16M4 18h10',
		bell: 'M6 9.5a6 6 0 1 1 12 0c0 4.5 2 5.5 2 6.5H4c0-1 2-2 2-6.5ZM10 19.5a2 2 0 0 0 4 0',
		alert: 'M12 4 2.8 19.5h18.4L12 4ZM12 10v4.5M12 17.2v.1',
		mute: 'M6 9.5a6 6 0 0 1 9.6-4.8M18 9.5c0 4.5 2 5.5 2 6.5H9M6 12c-.3 2.3-2 3-2 4h2M10 19.5a2 2 0 0 0 4 0M4 4l16 16',
		route: 'M4 7h4c4 0 4 10 8 10h4M4 17h4c1.3 0 2.2-1 3-2.3M20 7h-4c-1.3 0-2.2 1-3 2.3M17 4l3 3-3 3M17 14l3 3-3 3',
		template: 'M7 3h7l4 4v14H7V3ZM14 3v4h4M10 12h5M10 16h5',
		plug: 'M9 3v5M15 3v5M7 8h10v3a5 5 0 0 1-10 0V8ZM12 16v5',
		inbox: 'M4 13 6.5 5h11L20 13v6H4v-6ZM4 13h5a3 3 0 0 0 6 0h5',
		settings: 'M4 7h10M18 7h2M4 17h2M10 17h10M16 5v4M8 15v4',
		search: 'M11 18a7 7 0 1 0 0-14 7 7 0 0 0 0 14ZM20 20l-3.5-3.5',
		sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4',
		moon: 'M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5Z',
		logout: 'M10 4H5v16h5M14 8l4 4-4 4M18 12H9',
		left: 'M14.5 6 8.5 12l6 6',
		right: 'M9.5 6l6 6-6 6',
		first: 'M17 6l-6 6 6 6M7 6v12',
		last: 'M7 6l6 6-6 6M17 6v12',
		up: 'M12 19V5M6 11l6-6 6 6',
		down: 'M12 5v14M6 13l6 6 6-6',
		sort: 'M8 9l4-4 4 4M8 15l4 4 4-4',
		close: 'M6 6l12 12M18 6 6 18',
		pause: 'M9 5v14M15 5v14',
		play: 'M7 5l12 7-12 7V5Z',
		plus: 'M12 5v14M5 12h14'
	} as const;

	export type IconName = keyof typeof PATHS;
</script>

<script lang="ts">
	let { name, size = 16 }: { name: IconName; size?: number } = $props();
</script>

<!-- Decoration only: whatever it sits in carries the words. -->
<svg
	viewBox="0 0 24 24"
	width={size}
	height={size}
	fill="none"
	stroke="currentColor"
	stroke-width="1.75"
	stroke-linecap="round"
	stroke-linejoin="round"
	class="shrink-0"
	aria-hidden="true"
>
	<path d={PATHS[name]} />
</svg>
