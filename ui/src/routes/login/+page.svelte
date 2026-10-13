<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import LoginForm from '#lib/components/LoginForm.svelte';
</script>

<div class="mx-auto max-w-sm px-4 py-24">
	<h1 class="text-gold mb-8 text-2xl">Sign in to ramzes</h1>
	<LoginForm onsignedin={() => goto(resolve('/'), { refreshAll: true })} />
</div>
