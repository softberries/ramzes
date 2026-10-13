<script lang="ts">
	// Test-only wrapper: holds the bound form and prints the policy the editor produced.
	import type { Shaping } from '#lib/api/types.ts';
	import { formToShaping, shapingToForm, summarizeShaping } from '#lib/shaping.ts';
	import ShapingEditor from './ShapingEditor.svelte';

	let { initial }: { initial?: Shaping } = $props();

	// svelte-ignore state_referenced_locally
	let form = $state(shapingToForm(initial, 'Europe/Warsaw'));
</script>

<ShapingEditor bind:form />
<pre data-testid="shaping">{JSON.stringify(formToShaping(form))}</pre>
<p data-testid="summary">{summarizeShaping(formToShaping(form))}</p>
