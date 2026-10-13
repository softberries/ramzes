<script lang="ts">
	import { api, messageOf } from '#lib/api/client.ts';
	import type { ApiToken, Settings, TokenCreated, UserView } from '#lib/api/types.ts';
	import ConfigHistory from '#lib/components/ConfigHistory.svelte';
	import ConfigTransfer from '#lib/components/ConfigTransfer.svelte';
	import Confirm from '#lib/components/Confirm.svelte';
	import CopyBlock from '#lib/components/CopyBlock.svelte';
	import DataTable, { type Column } from '#lib/components/DataTable.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import PageHeader from '#lib/components/PageHeader.svelte';
	import { ago } from '#lib/format.ts';
	import { toasts } from '#lib/toasts.svelte.ts';

	let loaded = $state(false);
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
		loaded = true;
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

	const userColumns: Column<UserView>[] = [
		{ key: 'email', label: 'Email', sort: (u) => u.email },
		{ key: 'added', label: 'Added', sort: (u) => u.created_at, descendingFirst: true },
		{ key: 'actions', label: 'Actions', end: true }
	];

	const tokenColumns: Column<ApiToken>[] = [
		{ key: 'name', label: 'Name', sort: (t) => t.name },
		{ key: 'prefix', label: 'Starts with', sort: (t) => t.token_prefix, from: 'md' },
		{ key: 'used', label: 'Last used', sort: (t) => t.last_used_at, descendingFirst: true },
		{ key: 'created', label: 'Created', sort: (t) => t.created_at, descendingFirst: true, from: 'md' },
		{ key: 'actions', label: 'Actions', end: true }
	];
</script>

<PageHeader title="Settings" />

<div class="grid items-start gap-x-5 gap-y-8 xl:grid-cols-2">
	<div class="flex min-w-0 flex-col gap-8">
		<section class="flex flex-col gap-3" aria-label="Instance">
			<h2>Instance</h2>
			<form class="card" onsubmit={saveSettings}>
				<div class="card-body flex flex-col gap-5">
					<div class="max-w-md">
						<label class="field-label" for="public-url">Address of this ramzes instance</label>
						<input
							id="public-url"
							class="input mono"
							type="url"
							placeholder="https://ramzes.example.com"
							bind:value={publicUrl}
						/>
						<p class="hint mt-1">
							Used for the links in each message: back to the event, and to acknowledge it. While this is empty,
							messages carry neither link.
						</p>
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
								<input class="input mono" placeholder="Topic" aria-label="Topic" bind:value={override.topic} />
								<input class="input w-24 shrink-0" type="number" min="0" max="3650" aria-label="Days" bind:value={override.days} />
								<button
									type="button"
									class="btn btn-quiet btn-icon shrink-0"
									aria-label="Remove"
									onclick={() => (overrides = overrides.filter((_, i) => i !== index))}
								>
									<Icon name="close" />
								</button>
							</div>
						{/each}
						<div>
							<button type="button" class="btn" onclick={() => (overrides = [...overrides, { topic: '', days: 7 }])}>
								<Icon name="plus" />
								Add a topic
							</button>
						</div>
					</div>
					{#if settingsProblem}<p class="problem" role="alert">{settingsProblem}</p>{/if}
				</div>
				<div class="foot border-line border-t px-[1.1rem] py-3">
					<button type="submit" class="btn btn-primary">Save settings</button>
				</div>
			</form>
		</section>

		<ConfigTransfer onimport={() => imports++} />
	</div>

	<div class="flex min-w-0 flex-col gap-8">
		<section class="flex flex-col gap-3" aria-label="Users">
			<div>
				<h2>Users</h2>
				<p class="hint">Every user can change everything.</p>
			</div>
			<DataTable
				rows={users}
				columns={userColumns}
				key={(user) => user.id}
				noun="users"
				singular="user"
				search={(user) => user.email}
				initialSort={{ key: 'email', descending: false }}
				pageSize={10}
				{loaded}
			>
				{#snippet cell(user, column)}
					{#if column.key === 'email'}
						<span class="font-medium break-all">{user.email}</span>
					{:else if column.key === 'added'}
						<time class="text-muted whitespace-nowrap">{ago(user.created_at)}</time>
					{:else}
						<button type="button" class="btn btn-sm btn-danger" disabled={users.length === 1} onclick={() => (removingUser = user)}>
							Remove
						</button>
					{/if}
				{/snippet}
			</DataTable>
			<form class="card card-body flex flex-col gap-3" onsubmit={addUser}>
				<div class="grid gap-3 md:grid-cols-2">
					<div>
						<label class="field-label" for="user-email">Email</label>
						<input id="user-email" class="input" type="email" autocomplete="off" required bind:value={email} />
					</div>
					<div>
						<label class="field-label" for="user-password">Password</label>
						<input id="user-password" class="input" type="password" autocomplete="new-password" required bind:value={password} />
					</div>
				</div>
				<p class="hint">At least 10 characters. Share it with them yourself.</p>
				{#if userProblem}<p class="problem" role="alert">{userProblem}</p>{/if}
				<div><button type="submit" class="btn">Add user</button></div>
			</form>
		</section>

		<section class="flex flex-col gap-3" aria-label="API tokens">
			<div>
				<h2>API tokens</h2>
				<p class="hint">
					For scripts that manage ramzes. Send one as <code>Authorization: Bearer</code> followed by the token.
				</p>
			</div>
			{#if revealed}
				<div class="card card-body border-gold flex flex-col gap-3">
					<p>Copy the token for {revealed.name} now. It cannot be shown again.</p>
					<CopyBlock text={revealed.value} testid="token-value" />
					<div><button type="button" class="btn" onclick={() => (revealed = null)}>I have saved the token</button></div>
				</div>
			{/if}
			<DataTable
				rows={tokens}
				columns={tokenColumns}
				key={(token) => token.id}
				noun="tokens"
				singular="token"
				search={(token) => `${token.name} ${token.token_prefix}`}
				initialSort={{ key: 'name', descending: false }}
				pageSize={10}
				{loaded}
			>
				{#snippet cell(token, column)}
					{#if column.key === 'name'}
						<span class="font-medium break-words">{token.name}</span>
					{:else if column.key === 'prefix'}
						<span class="mono text-muted whitespace-nowrap">{token.token_prefix}…</span>
					{:else if column.key === 'used'}
						{#if token.last_used_at}
							<time class="text-muted whitespace-nowrap">{ago(token.last_used_at)}</time>
						{:else}
							<span class="text-faint whitespace-nowrap">never used</span>
						{/if}
					{:else if column.key === 'created'}
						<time class="text-muted whitespace-nowrap">{ago(token.created_at)}</time>
					{:else}
						<button type="button" class="btn btn-sm btn-danger" onclick={() => (revoking = token)}>Revoke</button>
					{/if}
				{/snippet}
				{#snippet empty()}
					No tokens.
				{/snippet}
			</DataTable>
			<form class="card card-body flex flex-col gap-3" onsubmit={createToken}>
				<div class="max-w-md">
					<label class="field-label" for="token-name">What the token is for</label>
					<input id="token-name" class="input" required maxlength="100" placeholder="deploy script" bind:value={tokenName} />
				</div>
				{#if tokenProblem}<p class="problem" role="alert">{tokenProblem}</p>{/if}
				<div><button type="submit" class="btn">Create token</button></div>
			</form>
		</section>
	</div>

	<div class="min-w-0 xl:col-span-2">
		{#key imports}<ConfigHistory />{/key}
	</div>
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

<style>
	.foot {
		background: color-mix(in srgb, var(--raised) 45%, transparent);
		border-radius: 0 0 var(--radius-card) var(--radius-card);
	}
</style>
