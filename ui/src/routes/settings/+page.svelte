<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ApiToken, Settings, TokenCreated, UserView } from '#lib/api/types.ts';
	import ConfigHistory from '#lib/components/ConfigHistory.svelte';
	import ConfigTransfer from '#lib/components/ConfigTransfer.svelte';
	import Confirm from '#lib/components/Confirm.svelte';
	import CopyBlock from '#lib/components/CopyBlock.svelte';
	import { ago } from '#lib/format.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	let retentionDays = $state(30);
	let overrides = $state<{ topic: string; days: number }[]>([]);
	let publicUrl = $state('');
	let settingsProblem = $state('');

	let users = $state<UserView[]>([]);
	let email = $state('');
	let password = $state('');
	let userProblem = $state('');
	let removingUser = $state<UserView | null>(null);

	let tokens = $state<ApiToken[]>([]);
	let tokenName = $state('');
	let tokenProblem = $state('');
	/** A token that was just created. It cannot be shown again. */
	let revealed = $state<TokenCreated | null>(null);
	let revoking = $state<ApiToken | null>(null);
	/** Goes up after an import, so the history is read again. */
	let imports = $state(0);

	function show(settings: Settings) {
		retentionDays = settings.retention_days;
		overrides = Object.entries(settings.retention_overrides).map(([topic, days]) => ({ topic, days }));
		publicUrl = settings.public_url ?? '';
	}

	async function load() {
		const [settings, userList, tokenList] = await Promise.all([
			api<Settings>('GET', '/api/settings'),
			api<UserView[]>('GET', '/api/users'),
			api<ApiToken[]>('GET', '/api/tokens')
		]);
		show(settings);
		users = userList;
		tokens = tokenList;
	}

	$effect(() => {
		load().catch((error) => toasts.problem(error));
	});

	async function saveSettings(event: SubmitEvent) {
		event.preventDefault();
		settingsProblem = '';
		try {
			const saved = await api<Settings>('PUT', '/api/settings', {
				retention_days: retentionDays,
				retention_overrides: Object.fromEntries(
					overrides.filter((o) => o.topic.trim() !== '').map((o) => [o.topic.trim(), o.days])
				),
				public_url: publicUrl.trim() || null
			});
			show(saved);
			toasts.ok('Settings saved.');
		} catch (error) {
			settingsProblem = messageOf(error);
		}
	}

	async function addUser(event: SubmitEvent) {
		event.preventDefault();
		userProblem = '';
		try {
			await api('POST', '/api/users', { email, password });
			toasts.ok('User added.');
			email = '';
			password = '';
			users = await api<UserView[]>('GET', '/api/users');
		} catch (error) {
			userProblem = messageOf(error);
		}
	}

	async function removeUser() {
		if (!removingUser) return;
		try {
			await api('DELETE', `/api/users/${removingUser.id}`);
			toasts.ok('User removed.');
			users = await api<UserView[]>('GET', '/api/users');
		} catch (error) {
			toasts.problem(error);
		} finally {
			removingUser = null;
		}
	}

	async function createToken(event: SubmitEvent) {
		event.preventDefault();
		tokenProblem = '';
		try {
			revealed = await api<TokenCreated>('POST', '/api/tokens', { name: tokenName });
			tokenName = '';
			tokens = await api<ApiToken[]>('GET', '/api/tokens');
		} catch (error) {
			tokenProblem = messageOf(error);
		}
	}

	async function revoke() {
		if (!revoking) return;
		try {
			await api('DELETE', `/api/tokens/${revoking.id}`);
			toasts.ok('Token revoked.');
			tokens = await api<ApiToken[]>('GET', '/api/tokens');
		} catch (error) {
			toasts.problem(error);
		} finally {
			revoking = null;
		}
	}
</script>

<h1 class="mb-6">Settings</h1>

<div class="flex max-w-3xl flex-col gap-12">
	<form class="flex flex-col gap-5" onsubmit={saveSettings}>
		<h2>Instance</h2>
		<div class="max-w-md">
			<label class="field-label" for="public-url">Address of this ramzes instance</label>
			<input id="public-url" class="input" type="url" placeholder="https://ramzes.example.com" bind:value={publicUrl} />
			<p class="hint mt-1">Used for the link back to the event in each message. Leave empty for no link.</p>
		</div>
		<div class="max-w-md">
			<label class="field-label" for="retention">Keep events for (days)</label>
			<input id="retention" class="input w-32" type="number" min="0" max="3650" required bind:value={retentionDays} />
			<p class="hint mt-1">0 keeps them forever.</p>
		</div>
		<div class="flex flex-col gap-2">
			<span class="field-label mb-0">Different retention for some topics</span>
			{#each overrides as override, index (index)}
				<div class="flex max-w-md gap-2">
					<input class="input" placeholder="Topic" aria-label="Topic" bind:value={override.topic} />
					<input class="input w-28" type="number" min="0" max="3650" aria-label="Days" bind:value={override.days} />
					<button type="button" class="btn btn-quiet" onclick={() => (overrides = overrides.filter((_, i) => i !== index))}>
						Remove
					</button>
				</div>
			{/each}
			<div>
				<button type="button" class="btn" onclick={() => (overrides = [...overrides, { topic: '', days: 7 }])}>
					Add a topic
				</button>
			</div>
		</div>
		{#if settingsProblem}<p class="problem" role="alert">{settingsProblem}</p>{/if}
		<div><button type="submit" class="btn btn-primary">Save settings</button></div>
	</form>

	<section class="flex flex-col gap-4">
		<h2>Users</h2>
		<p class="hint">Every user can change everything.</p>
		<ul class="border-line border-t">
			{#each users as user (user.id)}
				<li class="border-line flex flex-wrap items-baseline gap-x-4 border-b px-2 py-2">
					<span class="mr-auto font-medium">{user.email}</span>
					<span class="text-faint">added {ago(user.created_at)}</span>
					<button type="button" class="btn btn-danger h-7 px-2" disabled={users.length === 1} onclick={() => (removingUser = user)}>
						Remove
					</button>
				</li>
			{/each}
		</ul>
		<form class="flex max-w-md flex-col gap-3" onsubmit={addUser}>
			<div>
				<label class="field-label" for="user-email">Email</label>
				<input id="user-email" class="input" type="email" autocomplete="off" required bind:value={email} />
			</div>
			<div>
				<label class="field-label" for="user-password">Password</label>
				<input id="user-password" class="input" type="password" autocomplete="new-password" required bind:value={password} />
				<p class="hint mt-1">At least 10 characters. Share it with them yourself.</p>
			</div>
			{#if userProblem}<p class="problem" role="alert">{userProblem}</p>{/if}
			<div><button type="submit" class="btn">Add user</button></div>
		</form>
	</section>

	<section class="flex flex-col gap-4">
		<h2>API tokens</h2>
		<p class="hint">For scripts that manage ramzes. Send one as Authorization: Bearer followed by the token.</p>
		{#if revealed}
			<div class="panel border-gold flex flex-col gap-3 p-4">
				<p>Copy the token for {revealed.name} now. It cannot be shown again.</p>
				<CopyBlock text={revealed.value} testid="token-value" />
				<div><button type="button" class="btn" onclick={() => (revealed = null)}>I have saved the token</button></div>
			</div>
		{/if}
		<ul class="border-line border-t">
			{#each tokens as token (token.id)}
				<li class="border-line flex flex-wrap items-baseline gap-x-4 border-b px-2 py-2">
					<span class="font-medium">{token.name}</span>
					<span class="text-muted mr-auto">{token.token_prefix}…</span>
					<span class="text-faint">
						{token.last_used_at ? `used ${ago(token.last_used_at)}` : 'never used'}
					</span>
					<button type="button" class="btn btn-danger h-7 px-2" onclick={() => (revoking = token)}>Revoke</button>
				</li>
			{:else}
				<li class="hint border-line border-b px-2 py-2">No tokens.</li>
			{/each}
		</ul>
		<form class="flex max-w-md flex-col gap-3" onsubmit={createToken}>
			<div>
				<label class="field-label" for="token-name">What the token is for</label>
				<input id="token-name" class="input" required maxlength="100" placeholder="deploy script" bind:value={tokenName} />
			</div>
			{#if tokenProblem}<p class="problem" role="alert">{tokenProblem}</p>{/if}
			<div><button type="submit" class="btn">Create token</button></div>
		</form>
	</section>

	{#key imports}<ConfigHistory />{/key}

	<ConfigTransfer onimport={() => imports++} />
</div>

<Confirm
	open={removingUser !== null}
	title="Remove {removingUser?.email ?? 'this user'}?"
	action="Remove user"
	onconfirm={removeUser}
	oncancel={() => (removingUser = null)}
>
	They are signed out everywhere and can no longer sign in.
</Confirm>

<Confirm
	open={revoking !== null}
	title="Revoke {revoking?.name ?? 'this token'}?"
	action="Revoke token"
	onconfirm={revoke}
	oncancel={() => (revoking = null)}
>
	Anything still using it will be refused.
</Confirm>
