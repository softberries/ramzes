<script lang="ts">
	import { api } from '#lib/api/client.ts';
	import type { RevisionMeta, RevisionView } from '#lib/api/types.ts';
	import { lineDiff, nearChanges, type DiffGap, type DiffLine } from '#lib/diff.ts';
	import { ago } from '#lib/format.ts';
	import { toasts } from '#lib/toasts.svelte.ts';
	import Confirm from './Confirm.svelte';

	/** How many changes are listed before the rest is asked for. */
	const PAGE = 15;
	/** Unchanged lines shown on each side of a change. */
	const CONTEXT = 3;

	let revisions = $state<RevisionMeta[]>([]);
	let loaded = $state(false);
	let listed = $state(PAGE);
	/** The revision whose changes are shown, with those changes once loaded. */
	let shown = $state<{ id: string; lines: (DiffLine | DiffGap)[] | null } | null>(null);
	let rollingBack = $state<RevisionMeta | null>(null);

	async function load() {
		revisions = await api<RevisionMeta[]>('GET', '/api/config/revisions');
		loaded = true;
	}

	$effect(() => {
		load().catch((error) => toasts.problem(error));
	});

	async function show(revision: RevisionMeta) {
		if (shown?.id === revision.id) {
			shown = null;
			return;
		}
		shown = { id: revision.id, lines: null };
		// The list is newest first, so the configuration before this change is the next entry.
		const earlier = revisions[revisions.findIndex((r) => r.id === revision.id) + 1];
		try {
			const [after, before] = await Promise.all([
				api<RevisionView>('GET', `/api/config/revisions/${revision.id}`),
				earlier ? api<RevisionView>('GET', `/api/config/revisions/${earlier.id}`) : null
			]);
			if (shown?.id !== revision.id) return;
			shown.lines = nearChanges(lineDiff(before?.config ?? '', after.config), CONTEXT);
		} catch (error) {
			if (shown?.id === revision.id) shown = null;
			toasts.problem(error);
		}
	}

	async function rollBack() {
		if (!rollingBack) return;
		try {
			await api('POST', `/api/config/revisions/${rollingBack.id}/rollback`, {});
			toasts.ok('Configuration rolled back.');
			shown = null;
			await load();
		} catch (error) {
			toasts.problem(error);
		} finally {
			rollingBack = null;
		}
	}
</script>

<section class="flex flex-col gap-4" aria-label="Configuration history">
	<h2>Configuration history</h2>
	<p class="hint">
		Every change to routes, templates, connections, connectors and sources is recorded here, newest first.
	</p>
	<ul class="border-line border-t">
		{#each revisions.slice(0, listed) as revision, index (revision.id)}
			<li class="border-line border-b px-2 py-2">
				<div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
					<span class="mr-auto min-w-0 font-medium break-words">{revision.summary || 'Configuration changed'}</span>
					<span class="text-muted break-all">{revision.author}</span>
					<span class="text-faint">{ago(revision.created_at)}</span>
					<button
						type="button"
						class="btn h-7 px-2"
						aria-expanded={shown?.id === revision.id}
						onclick={() => show(revision)}
					>
						{shown?.id === revision.id ? 'Hide changes' : 'Show changes'}
					</button>
				</div>
				{#if shown?.id === revision.id && shown.lines}
					<div class="mt-2 flex flex-col gap-2">
						{#if shown.lines.length === 0}
							<p class="hint">The configuration is the same as before this entry.</p>
						{:else}
							<pre class="code max-h-[28rem] overflow-auto" data-testid="config-diff">{#each shown.lines as line, at (at)}{#if line.kind === 'gap'}<span
											class="text-faint block">  … {line.count} unchanged {line.count === 1 ? 'line' : 'lines'}</span
										>{:else}<span
											class="block {line.kind === 'added' ? 'text-ok' : line.kind === 'removed' ? 'text-bad' : 'text-muted'}"
											data-kind={line.kind}
											>{line.kind === 'added' ? '+ ' : line.kind === 'removed' ? '- ' : '  '}{line.text}</span
										>{/if}{/each}</pre>
						{/if}
						{#if index > 0}
							<div>
								<button type="button" class="btn btn-danger" onclick={() => (rollingBack = revision)}>
									Roll back to this
								</button>
							</div>
						{:else}
							<p class="hint">This is the current configuration.</p>
						{/if}
					</div>
				{/if}
			</li>
		{:else}
			{#if loaded}<li class="hint border-line border-b px-2 py-2">Nothing has been changed yet.</li>{/if}
		{/each}
	</ul>
	{#if revisions.length > listed}
		<div><button type="button" class="btn" onclick={() => (listed += PAGE)}>Show older changes</button></div>
	{/if}
</section>

<Confirm
	open={rollingBack !== null}
	title="Roll back the configuration?"
	action="Roll back"
	onconfirm={rollBack}
	oncancel={() => (rollingBack = null)}
>
	Routes, templates, connections and connectors go back to how they were after “{rollingBack?.summary}”.
	Connections keep or regain the credentials they had at that time. Source keys are not touched. The
	rollback is itself recorded, so it can be undone.
</Confirm>
