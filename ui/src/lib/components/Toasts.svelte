<script lang="ts">
	import { toasts } from '#lib/toasts.svelte.ts';
</script>

<div class="fixed right-4 bottom-4 z-50 flex max-w-md flex-col gap-2" aria-live="polite">
	{#each toasts.list as toast (toast.id)}
		<button
			type="button"
			class="panel px-3 py-2 text-left {toast.kind === 'problem' ? 'problem border-bad' : 'border-ok'}"
			onclick={() => toasts.dismiss(toast.id)}
		>
			{toast.text}
		</button>
	{/each}
</div>
