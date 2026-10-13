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

<dialog bind:this={dialog} class="panel text-fg m-auto w-full max-w-md p-5 backdrop:bg-black/50" onclose={oncancel}>
	<h2 class="mb-2">{title}</h2>
	<div class="text-muted mb-5">{@render children?.()}</div>
	<div class="flex justify-end gap-2">
		<button type="button" class="btn" onclick={oncancel}>Cancel</button>
		<button type="button" class="btn btn-danger" onclick={onconfirm}>{action}</button>
	</div>
</dialog>
