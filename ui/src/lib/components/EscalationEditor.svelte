<script lang="ts">
	import type { ConnectionView, Template } from '#lib/api/types.ts';
	import { cumulativeSeconds, delaysOf, emptyStep, sinceFirstWords, type StepForm } from '#lib/escalation.ts';
	import { UNITS } from '#lib/shaping.ts';
	import TargetRows from './TargetRows.svelte';

	interface Props {
		/** The chain being edited, in order. Turn it into what the API takes with `formToChain`. */
		steps: StepForm[];
		connections: ConnectionView[];
		templates: Template[];
		/** What a connection's connector calls its destination, or null when it takes none. */
		destinationLabel: (connectionId: string) => string | null;
	}

	let { steps = $bindable(), connections, templates, destinationLabel }: Props = $props();

	const since = $derived(cumulativeSeconds(delaysOf(steps)));
</script>

<div class="flex flex-col gap-3">
	<p class="hint max-w-2xl">
		While a sent notification is not acknowledged, each step tells someone else. The delay of a step counts
		from the step before it, and the first from the first message.
	</p>
	{#if steps.length > 0}
		<ol class="flex flex-col gap-4" aria-label="Escalation steps">
			{#each steps as step, index (index)}
				<li class="flex flex-col gap-2" aria-label="Step {index + 1}">
					<div class="flex flex-wrap items-center gap-2">
						<span class="font-medium">{index + 1}.</span>
						<span>after</span>
						<input
							class="input w-20"
							type="number"
							min="1"
							step="1"
							inputmode="numeric"
							required
							aria-label="Step {index + 1}: delay"
							bind:value={step.after.amount}
						/>
						<select class="input w-28" aria-label="Step {index + 1}: unit" bind:value={step.after.unit}>
							{#each UNITS as unit (unit.unit)}<option value={unit.unit}>{unit.label}</option>{/each}
						</select>
						<span>tell</span>
						{#if since[index] > 0}
							<span class="text-muted" data-testid="since-first">{sinceFirstWords(since[index])}</span>
						{/if}
						<button
							type="button"
							class="btn btn-quiet ml-auto"
							aria-label="Remove step {index + 1}"
							onclick={() => (steps = steps.filter((_, i) => i !== index))}
						>
							Remove step
						</button>
					</div>
					<div class="part">
						<TargetRows
							bind:targets={step.targets}
							{connections}
							{templates}
							{destinationLabel}
							prefix="Step {index + 1}: "
						/>
					</div>
				</li>
			{/each}
		</ol>
	{/if}
	<div>
		<button type="button" class="btn" onclick={() => (steps = [...steps, emptyStep()])}>
			{steps.length === 0 ? 'Add a step' : 'Add another step'}
		</button>
	</div>
</div>
