<script lang="ts">
	import { SEVERITIES, type LabelCond, type RouteMatch } from '#lib/api/types.ts';
	import Icon from './Icon.svelte';

	interface Props {
		/** The match being edited. Clean it with `cleanMatch` before sending it. */
		match: RouteMatch;
		sources: { id: string; name: string }[];
	}

	let { match = $bindable(), sources }: Props = $props();

	const labels = $derived(match.labels ?? []);

	function setLabels(next: LabelCond[]) {
		match = { ...match, labels: next };
	}

	function add() {
		setLabels([...labels, { op: 'equals', key: '', value: '' }]);
	}

	function remove(index: number) {
		setLabels(labels.filter((_, i) => i !== index));
	}

	/** Replaces one condition, keeping its key when the kind changes. */
	function change(index: number, patch: { op?: LabelCond['op']; key?: string; text?: string }) {
		const current = labels[index];
		const op = patch.op ?? current.op;
		const key = patch.key ?? current.key;
		const text = patch.text ?? ('value' in current ? current.value : 'pattern' in current ? current.pattern : '');
		const next: LabelCond =
			op === 'equals' ? { op, key, value: text } : op === 'matches' ? { op, key, pattern: text } : { op, key };
		setLabels(labels.map((cond, i) => (i === index ? next : cond)));
	}

	const value = (event: Event) => (event.currentTarget as HTMLInputElement | HTMLSelectElement).value;
</script>

<div class="flex flex-col gap-4">
	<div class="grid gap-4 md:grid-cols-3">
		<div>
			<label class="field-label" for="match-topic">Topic</label>
			<input
				id="match-topic"
				class="input"
				placeholder="Any topic"
				value={match.topic ?? ''}
				oninput={(e) => (match = { ...match, topic: value(e) || null })}
			/>
			<p class="hint mt-1"><code>*</code> matches any run of characters, as in <code>orders.*</code></p>
		</div>
		<div>
			<label class="field-label" for="match-severity">Minimum severity</label>
			<select
				id="match-severity"
				class="input"
				value={match.min_severity ?? ''}
				onchange={(e) => (match = { ...match, min_severity: (value(e) || null) as RouteMatch['min_severity'] })}
			>
				<option value="">Any severity</option>
				{#each SEVERITIES.slice(1) as severity (severity)}<option value={severity}>{severity}</option>{/each}
			</select>
		</div>
		<div>
			<label class="field-label" for="match-source">Source</label>
			<select
				id="match-source"
				class="input"
				value={match.source_id ?? ''}
				onchange={(e) => (match = { ...match, source_id: value(e) || null })}
			>
				<option value="">Any source</option>
				{#each sources as source (source.id)}<option value={source.id}>{source.name}</option>{/each}
			</select>
		</div>
	</div>

	<div class="flex flex-col gap-2">
		{#each labels as cond, index (index)}
			<div class="grid grid-cols-[1fr_auto] gap-2 md:grid-cols-[1fr_9rem_1fr_auto]">
				<input
					class="input"
					placeholder="Label key"
					aria-label="Label key"
					value={cond.key}
					oninput={(e) => change(index, { key: value(e) })}
				/>
				<select
					class="input"
					aria-label="Condition"
					value={cond.op}
					onchange={(e) => change(index, { op: value(e) as LabelCond['op'] })}
				>
					<option value="equals">equals</option>
					<option value="matches">matches</option>
					<option value="exists">is present</option>
				</select>
				{#if cond.op === 'equals'}
					<input
						class="input"
						placeholder="Value"
						aria-label="Label value"
						value={cond.value}
						oninput={(e) => change(index, { text: value(e) })}
					/>
				{:else if cond.op === 'matches'}
					<input
						class="input"
						placeholder="Regular expression"
						aria-label="Pattern"
						value={cond.pattern}
						oninput={(e) => change(index, { text: value(e) })}
					/>
				{:else}
					<span class="hidden md:block"></span>
				{/if}
				<button type="button" class="btn btn-quiet btn-icon" aria-label="Remove condition" onclick={() => remove(index)}>
					<Icon name="close" />
				</button>
			</div>
		{/each}
		<div>
			<button type="button" class="btn" onclick={add}>
				<Icon name="plus" />
				Add label condition
			</button>
		</div>
	</div>

	<div>
		<label class="field-label" for="match-expr">Expression</label>
		<input
			id="match-expr"
			class="input mono"
			placeholder="No expression"
			autocomplete="off"
			spellcheck="false"
			value={match.expr ?? ''}
			oninput={(e) => (match = { ...match, expr: value(e) || null })}
		/>
		<p class="hint mt-1">
			Optional. The event must also pass this test, which can read topic, severity, status, title, labels and
			payload. For example: <code>payload.amount &gt; 100 and labels.env == 'prod'</code>
		</p>
	</div>
</div>
