<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import { toasts } from '#lib/toasts.svelte.ts';
	import Confirm from './Confirm.svelte';

	/** Called after a configuration was imported. */
	let { onimport }: { onimport: () => void } = $props();

	let yaml = $state('');
	let problem = $state('');
	let confirming = $state(false);
	let busy = $state(false);

	async function download() {
		try {
			const exported = await api<{ yaml: string }>('GET', '/api/config/export');
			const url = URL.createObjectURL(new Blob([exported.yaml], { type: 'application/yaml' }));
			const link = document.createElement('a');
			link.href = url;
			link.download = 'ramzes.yaml';
			link.click();
			URL.revokeObjectURL(url);
		} catch (error) {
			toasts.problem(error);
		}
	}

	async function readFile(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;
		yaml = await file.text();
		problem = '';
		// The same file can be chosen again after it was edited.
		input.value = '';
	}

	async function importNow() {
		confirming = false;
		busy = true;
		problem = '';
		try {
			await api('POST', '/api/config/import', { yaml });
			toasts.ok('Configuration imported.');
			yaml = '';
			onimport();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}
</script>

<section class="flex flex-col gap-3" aria-label="Export and import">
	<div>
		<h2>Export and import</h2>
		<p class="hint">
			The configuration as one YAML file: routes, templates, connections, connectors, source mappings, and the
			instance's address and retention. It holds no credentials and no source keys, so it can be kept in
			version control.
		</p>
	</div>
	<div class="card">
		<div class="card-body"><button type="button" class="btn" onclick={download}>Download ramzes.yaml</button></div>

		<form
			class="card-body border-line flex flex-col gap-3 border-t"
			onsubmit={(event) => {
				event.preventDefault();
				confirming = true;
			}}
		>
			<div>
				<label class="field-label" for="import-file">Import from a file</label>
				<input id="import-file" class="file-input max-w-full" type="file" accept=".yaml,.yml,text/yaml,application/yaml" onchange={readFile} />
			</div>
			<div>
				<label class="field-label" for="import-yaml">Or paste the YAML</label>
				<textarea
					id="import-yaml"
					class="input h-40 py-2 whitespace-pre"
					spellcheck="false"
					required
					bind:value={yaml}
				></textarea>
			</div>
			{#if problem}
				<p class="problem break-words" role="alert">Nothing was changed. {problem}</p>
			{/if}
			<div><button type="submit" class="btn" disabled={busy}>Import configuration</button></div>
		</form>
	</div>
</section>

<Confirm
	open={confirming}
	title="Replace the configuration?"
	action="Replace configuration"
	onconfirm={importNow}
	oncancel={() => (confirming = false)}
>
	This replaces all routes, templates, connections and connector definitions with the ones in the file, and
	sets the names and mappings of the sources it lists and the instance's address and retention. Connections
	that already exist keep their credentials;
	new ones start without any. The change is recorded in the history, so it can be rolled back.
</Confirm>
