<script lang="ts">
	// Test-only wrapper: holds the bound match and prints what the editor produced.
	import type { RouteMatch } from '#lib/api/types.ts';
	import { cleanMatch, summarize } from '#lib/routes.ts';
	import MatchEditor from './MatchEditor.svelte';

	let { initial, sources }: { initial: RouteMatch; sources: { id: string; name: string }[] } = $props();

	// svelte-ignore state_referenced_locally
	let match = $state<RouteMatch>(structuredClone(initial));
	const names = $derived(Object.fromEntries(sources.map((s) => [s.id, s.name])));
</script>

<MatchEditor bind:match {sources} />
<pre data-testid="match">{JSON.stringify(cleanMatch(match))}</pre>
<p data-testid="summary">{summarize(cleanMatch(match), names)}</p>
