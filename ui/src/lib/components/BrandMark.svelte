<script lang="ts">
	let { size = 40 }: { size?: number } = $props();
</script>

<!-- The pyramid of the navigation rail, for the screens that have no rail. -->
<svg viewBox="0 0 32 32" width={size} height={size} class="text-gold shrink-0" aria-hidden="true">
	<path d="M16 5 L28 26 H4 Z" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linejoin="round" />
	<path d="M16 15 L22 26 H10 Z" fill="currentColor" />
</svg>
