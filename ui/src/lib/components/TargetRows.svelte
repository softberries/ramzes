<script lang="ts">
	import type { ConnectionView, Template } from '#lib/api/types.ts';
	import { emptyTarget, type TargetRow } from '#lib/escalation.ts';

	interface Props {
		/** The rows being edited. Clean them with `cleanTargets` before sending. */
		targets: TargetRow[];
		connections: ConnectionView[];
		templates: Template[];
		/** What a connection's connector calls its destination, or null when it takes none. */
		destinationLabel: (connectionId: string) => string | null;
		/** Put before the name of each field, to tell the rows of one list from another's. */
		prefix?: string;
		/** Name the three columns above the rows on a wide screen. */
		header?: boolean;
	}

	let { targets = $bindable(), connections, templates, destinationLabel, prefix = '', header = false }: Props = $props();
</script>

<div class="flex flex-col gap-2">
	{#if header}
		<div class="text-muted hidden gap-2 md:grid md:grid-cols-[1fr_1fr_1fr_5rem]" aria-hidden="true">
			<span>Connection</span>
			<span>Destination</span>
			<span>Wording</span>
		</div>
	{/if}
	{#each targets as target, index (index)}
		{@const label = destinationLabel(target.connection_id)}
		<div class="border-line grid gap-2 border-b pb-2 md:grid-cols-[1fr_1fr_1fr_5rem] md:border-b-0 md:pb-0">
			<select class="input" aria-label="{prefix}Connection" bind:value={target.connection_id} required>
				<option value="" disabled>Choose a connection</option>
				{#each connections as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
			</select>
			{#if label}
				<input class="input" aria-label="{prefix}{label}" placeholder="{label} (optional)" bind:value={target.destination} />
			{:else}
				<span class="text-faint hidden self-center md:block">The connection's own</span>
			{/if}
			<select class="input" aria-label="{prefix}Template" bind:value={target.template_id}>
				<option value="">The event's own wording</option>
				{#each templates as template (template.id)}<option value={template.id}>{template.name}</option>{/each}
			</select>
			{#if targets.length > 1}
				<button
					type="button"
					class="btn btn-quiet justify-self-start"
					onclick={() => (targets = targets.filter((_, i) => i !== index))}
				>
					Remove
				</button>
			{/if}
		</div>
	{/each}
	{#if connections.length > 0}
		<div>
			<button type="button" class="btn" onclick={() => (targets = [...targets, emptyTarget()])}>
				Add another connection
			</button>
		</div>
	{/if}
</div>
