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

<section class="flex flex-col gap-3" aria-label="Configuration history">
	<div>
		<h2>Configuration history</h2>
		<p class="hint">
			Every change to routes, templates, connections, connectors and sources is recorded here, newest first.
		</p>
	</div>
	<div class="card overflow-hidden">
		<ul class="revisions">
			{#each revisions.slice(0, listed) as revision, index (revision.id)}
				<li class="px-[1.1rem] py-2.5">
					<div class="flex flex-wrap items-center gap-x-4 gap-y-1">
						<span class="w-full min-w-0 font-medium break-words md:w-auto md:flex-1">{revision.summary || 'Configuration changed'}</span>
						<span class="text-muted break-all">{revision.author}</span>
						<time class="text-faint whitespace-nowrap">{ago(revision.created_at)}</time>
						<button
							type="button"
							class="btn btn-sm ml-auto md:ml-0"
							aria-expanded={shown?.id === revision.id}
							onclick={() => show(revision)}
						>
							{shown?.id === revision.id ? 'Hide changes' : 'Show changes'}
						</button>
					</div>
					{#if shown?.id === revision.id && shown.lines}
						<div class="mt-2.5 mb-1 flex flex-col gap-2.5">
							{#if shown.lines.length === 0}
								<p class="hint">The configuration is the same as before this entry.</p>
							{:else}
								<pre class="code max-h-[28rem] overflow-auto" data-testid="config-diff">{#each shown.lines as line, at (at)}{#if line.kind === 'gap'}<span
												class="text-faint block">  … {line.count} unchanged {line.count === 1 ? 'line' : 'lines'}</span
											>{:else}<span
												class="line block {line.kind === 'added' ? 'text-ok' : line.kind === 'removed' ? 'text-bad' : 'text-muted'}"
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
				{#if loaded}<li class="hint px-[1.1rem] py-10 text-center">Nothing has been changed yet.</li>{/if}
			{/each}
		</ul>
		{#if revisions.length > listed}
			<div class="border-line border-t px-[1.1rem] py-2.5">
				<button type="button" class="btn btn-sm" onclick={() => (listed += PAGE)}>Show older changes</button>
			</div>
		{/if}
	</div>
</section>

<Confirm
	open={rollingBack !== null}
	title="Roll back the configuration?"
	action="Roll back"
	onconfirm={rollBack}
	oncancel={() => (rollingBack = null)}
>
	Routes, templates, connections, connectors and the instance's address and retention go back to how they
	were after “{rollingBack?.summary}”.
	A connection that still exists keeps the credentials it has now; one that had been deleted comes back with the
	credentials it had then. Source keys are not touched. The
	rollback is itself recorded, so it can be undone.
</Confirm>

<style>
	.revisions > li + li {
		border-top: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
	}

	/* A changed line is tinted across the whole block, so what was added and removed stands out when scanning. */
	.line[data-kind='added'] {
		background: color-mix(in srgb, var(--st-delivered) 10%, transparent);
	}

	.line[data-kind='removed'] {
		background: color-mix(in srgb, var(--st-failed) 10%, transparent);
	}
</style>
