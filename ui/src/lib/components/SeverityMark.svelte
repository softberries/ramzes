<script lang="ts">
	import type { Severity } from '#lib/api/types.ts';

	let { severity, label = false }: { severity: Severity; label?: boolean } = $props();
</script>

<!-- A square marks severity; the word is always available to assistive technology. -->
<span class="inline-flex items-center gap-2" style="color: var(--sev-{severity})" data-severity={severity}>
	<span class="inline-block h-2 w-2 shrink-0 bg-current" aria-hidden="true"></span>
	<span class={label ? '' : 'sr-only'}>{severity}</span>
</span>
