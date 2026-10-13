<script lang="ts">
	import { toasts } from '#lib/toasts.svelte.ts';
</script>

<!-- The strip itself lets clicks through; only a toast takes them. -->
<div
	class="pointer-events-none fixed right-4 bottom-4 left-4 z-50 flex flex-col items-end gap-2 md:left-auto md:max-w-md"
	aria-live="polite"
>
	{#each toasts.list as toast (toast.id)}
		<button
			type="button"
			class="card toast pointer-events-auto flex max-w-full items-baseline gap-2.5 py-2.5 pr-4 pl-3.5 text-left"
			style="--tone: var(--st-{toast.kind === 'problem' ? 'failed' : 'delivered'})"
			onclick={() => toasts.dismiss(toast.id)}
		>
			<span class="mark" aria-hidden="true"></span>
			<span class="min-w-0 break-words">{toast.text}</span>
		</button>
	{/each}
</div>

<style>
	.toast {
		border-left: 3px solid var(--tone);
	}

	.mark {
		flex: none;
		width: 0.5rem;
		height: 0.5rem;
		border-radius: 50%;
		background: var(--tone);
	}
</style>
