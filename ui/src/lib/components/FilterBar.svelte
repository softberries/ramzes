<script lang="ts">
	import { OUTCOMES, SEVERITIES, type Source } from '#lib/api/types.ts';
	import type { FeedFilters } from '#lib/feed.ts';

	interface Props {
		filters: FeedFilters;
		sources: Source[];
		/** Called with the complete new set of filters. */
		onchange: (filters: FeedFilters) => void;
	}

	let { filters, sources, onchange }: Props = $props();

	const RANGES = [
		{ label: 'Last 15 minutes', ms: 15 * 60_000 },
		{ label: 'Last hour', ms: 3_600_000 },
		{ label: 'Last 24 hours', ms: 86_400_000 },
		{ label: 'Last 7 days', ms: 7 * 86_400_000 }
	];
	const OUTCOME_WORDS = { pending: 'Pending', delivered: 'Delivered', failed: 'Failed', unrouted: 'No route' };

	function patch(change: Partial<FeedFilters>) {
		onchange({ ...filters, ...change });
	}

	function text(key: 'q' | 'topic' | 'label') {
		return (event: Event) => patch({ [key]: (event.currentTarget as HTMLInputElement).value || undefined });
	}

	function choice<K extends 'source' | 'min_severity' | 'outcome'>(key: K) {
		return (event: Event) =>
			patch({ [key]: ((event.currentTarget as HTMLSelectElement).value || undefined) as FeedFilters[K] });
	}

	function range(event: Event) {
		const ms = Number((event.currentTarget as HTMLSelectElement).value);
		patch({ since: ms ? Date.now() - ms : undefined, until: undefined });
	}
</script>

<div class="grid grid-cols-2 gap-2 md:grid-cols-4 xl:grid-cols-8">
	<input
		class="input col-span-2 xl:col-span-2"
		type="search"
		placeholder="Find in title"
		aria-label="Find in title"
		value={filters.q ?? ''}
		onchange={text('q')}
	/>
	<input
		class="input"
		placeholder="Topic: orders.*"
		aria-label="Topic"
		value={filters.topic ?? ''}
		onchange={text('topic')}
	/>
	<input
		class="input"
		placeholder="Label: env=prod"
		aria-label="Label"
		value={filters.label ?? ''}
		onchange={text('label')}
	/>
	<select class="input" aria-label="Source" value={filters.source ?? ''} onchange={choice('source')}>
		<option value="">Any source</option>
		{#each sources as source (source.id)}<option value={source.id}>{source.name}</option>{/each}
	</select>
	<select class="input" aria-label="Minimum severity" value={filters.min_severity ?? ''} onchange={choice('min_severity')}>
		<option value="">Any severity</option>
		{#each SEVERITIES.slice(1) as severity (severity)}<option value={severity}>{severity} and above</option>{/each}
	</select>
	<select class="input" aria-label="Outcome" value={filters.outcome ?? ''} onchange={choice('outcome')}>
		<option value="">Any outcome</option>
		{#each OUTCOMES as outcome (outcome)}<option value={outcome}>{OUTCOME_WORDS[outcome]}</option>{/each}
	</select>
	<select class="input" aria-label="Time range" onchange={range}>
		<option value="0">{filters.since ? 'Custom range' : 'Any time'}</option>
		{#each RANGES as r (r.ms)}<option value={r.ms}>{r.label}</option>{/each}
	</select>
</div>
