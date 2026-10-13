<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import BrandMark from '#lib/components/BrandMark.svelte';
	import LoginForm from '#lib/components/LoginForm.svelte';
</script>

<div class="flex min-h-dvh items-center justify-center px-4 py-10">
	<div class="w-full max-w-sm">
		<div class="mb-6 flex flex-col items-center gap-3 text-center">
			<BrandMark />
			<h1>Sign in to ramzes</h1>
		</div>
		<div class="card p-6">
			<LoginForm onsignedin={() => goto(resolve('/'), { refreshAll: true })} />
		</div>
	</div>
</div>
