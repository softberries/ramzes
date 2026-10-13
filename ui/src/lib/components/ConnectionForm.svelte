<script lang="ts">
	import type { ConnectorDescriptor } from '#lib/api/types.ts';

	/** What the form submits. `credentials` is absent when editing and no secret was typed. */
	export interface ConnectionValues {
		name: string;
		settings: Record<string, string>;
		credentials?: Record<string, string>;
	}

	interface Props {
		descriptor: ConnectorDescriptor;
		onsubmit: (values: ConnectionValues) => void;
		submitLabel: string;
		/** Present when editing an existing connection, whose secrets cannot be read back. */
		initial?: { name: string; settings: Record<string, unknown> };
		/** Shows a Cancel button beside the submit button. */
		oncancel?: () => void;
		busy?: boolean;
		problem?: string;
	}

	let { descriptor, onsubmit, submitLabel, initial, oncancel, busy = false, problem = '' }: Props = $props();

	const editing = $derived(initial !== undefined);
	// svelte-ignore state_referenced_locally
	let name = $state(initial?.name ?? descriptor.name);
	// svelte-ignore state_referenced_locally
	let values = $state<Record<string, string>>(
		Object.fromEntries(
			descriptor.fields.map((f) => [f.key, f.secret ? '' : String(initial?.settings[f.key] ?? '')])
		)
	);

	function submit(event: SubmitEvent) {
		event.preventDefault();
		const pick = (secret: boolean) =>
			Object.fromEntries(
				descriptor.fields.filter((f) => f.secret === secret && values[f.key] !== '').map((f) => [f.key, values[f.key]])
			);
		const credentials = pick(true);
		const keepStored = editing && Object.keys(credentials).length === 0;
		onsubmit({ name, settings: pick(false), ...(keepStored ? {} : { credentials }) });
	}
</script>

<form class="flex flex-col gap-4" onsubmit={submit}>
	<div>
		<label class="field-label" for="connection-name">Name</label>
		<input id="connection-name" class="input" bind:value={name} required maxlength="100" />
	</div>
	{#each descriptor.fields as field (field.key)}
		<div>
			<label class="field-label" for="connection-{field.key}">{field.label}</label>
			<input
				id="connection-{field.key}"
				class="input"
				type={field.secret ? 'password' : field.kind === 'url' ? 'url' : 'text'}
				autocomplete="off"
				required={field.required && !(editing && field.secret)}
				bind:value={values[field.key]}
			/>
			<p class="hint mt-1">
				{field.help}
				{#if editing && field.secret}Leave empty to keep the stored value.{/if}
			</p>
		</div>
	{/each}
	{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
	<div class="flex gap-2">
		<button type="submit" class="btn btn-primary" disabled={busy}>{submitLabel}</button>
		{#if oncancel}<button type="button" class="btn" onclick={oncancel}>Cancel</button>{/if}
	</div>
</form>
