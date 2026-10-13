<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';

	let { onsignedin }: { onsignedin: () => void } = $props();

	let email = $state('');
	let password = $state('');
	let busy = $state(false);
	let problem = $state('');

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		problem = '';
		try {
			await api('POST', '/api/auth/login', { email, password });
			onsignedin();
		} catch (error) {
			problem = messageOf(error);
		} finally {
			busy = false;
		}
	}
</script>

<form class="flex flex-col gap-4" onsubmit={submit}>
	<div>
		<label class="field-label" for="login-email">Email</label>
		<input id="login-email" class="input" type="email" autocomplete="username" required bind:value={email} />
	</div>
	<div>
		<label class="field-label" for="login-password">Password</label>
		<input
			id="login-password"
			class="input"
			type="password"
			autocomplete="current-password"
			required
			bind:value={password}
		/>
	</div>
	{#if problem}<p class="problem" role="alert">{problem}</p>{/if}
	<div>
		<button type="submit" class="btn btn-primary" disabled={busy}>Sign in</button>
	</div>
</form>
