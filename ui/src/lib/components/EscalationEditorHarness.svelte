<script lang="ts">
	// Test-only wrapper: holds the bound steps and prints the chain the editor produced.
	import type { ConnectionView, EscalationStep, Template } from '#lib/api/types.ts';
	import { chainToForm, formToChain, summarizeChain } from '#lib/escalation.ts';
	import EscalationEditor from './EscalationEditor.svelte';

	interface Props {
		initial?: EscalationStep[];
		connections: ConnectionView[];
		templates?: Template[];
		/** Destination labels by connection id; a connection that is not listed takes no destination. */
		destinations?: Record<string, string>;
	}

	let { initial, connections, templates = [], destinations = {} }: Props = $props();

	// svelte-ignore state_referenced_locally
	let steps = $state(chainToForm(initial));

	const names = $derived(Object.fromEntries(connections.map((c) => [c.id, c.name])));
	const chain = $derived(formToChain(steps, (id) => id in destinations));
</script>

<EscalationEditor bind:steps {connections} {templates} destinationLabel={(id) => destinations[id] ?? null} />
<pre data-testid="chain">{JSON.stringify(chain)}</pre>
<p data-testid="summary">{summarizeChain(chain, names)}</p>
