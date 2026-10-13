<script lang="ts">
	import type { Snippet } from 'svelte';

	interface Props {
		open: boolean;
		title: string;
		/** The label of the button that goes ahead, naming the action. */
		action: string;
		onconfirm: () => void;
		oncancel: () => void;
		children?: Snippet;
	}

	let { open, title, action, onconfirm, oncancel, children }: Props = $props();

	let dialog = $state<HTMLDialogElement>();

	$effect(() => {
		if (open && !dialog?.open) dialog?.showModal();
		if (!open && dialog?.open) dialog.close();
	});
</script>

<dialog
	bind:this={dialog}
	class="card text-fg m-auto w-[calc(100%-2rem)] max-w-md overflow-hidden p-0 backdrop:bg-black/50"
	onclose={oncancel}
>
	<div class="px-5 pt-5 pb-4">
		<h2 class="mb-2">{title}</h2>
		<div class="text-muted">{@render children?.()}</div>
	</div>
	<div class="foot border-line flex justify-end gap-2 border-t px-5 py-3">
		<button type="button" class="btn" onclick={oncancel}>Cancel</button>
		<button type="button" class="btn btn-danger" onclick={onconfirm}>{action}</button>
	</div>
</dialog>

<style>
	.foot {
		background: color-mix(in srgb, var(--raised) 45%, transparent);
	}
</style>
