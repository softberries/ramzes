import { beforeEach, describe, expect, it, vi } from 'vitest';
import { page } from 'vitest/browser';
import { render } from 'vitest-browser-svelte';
import type { ConnectorDescriptor } from '#lib/api/types.ts';

const { apiMock } = vi.hoisted(() => ({ apiMock: vi.fn() }));
vi.mock('#lib/api/client.ts', async (original) => ({
	...(await original<typeof import('#lib/api/client.ts')>()),
	api: apiMock
}));

import { ApiError } from '#lib/api/client.ts';
import ConnectionForm from './ConnectionForm.svelte';
import LoginForm from './LoginForm.svelte';
import SetupFlow from './SetupFlow.svelte';

const WEBHOOK: ConnectorDescriptor = {
	kind: 'webhook',
	name: 'Webhook',
	description: 'POST each message as JSON to a URL of your choice.',
	capabilities: { rich: false, update: false, threads: false, buttons: false, max_len: 20000 },
	destination_label: null,
	fields: [
		{ key: 'url', label: 'URL', help: 'Where to send the request.', secret: false, required: true, kind: 'url' },
		{ key: 'secret', label: 'Signing secret', help: 'Signs each request.', secret: true, required: false, kind: 'text' }
	]
};

beforeEach(() => {
	apiMock.mockReset();
});

describe('ConnectionForm', () => {
	it('renders one input per field and masks secrets', async () => {
		render(ConnectionForm, { descriptor: WEBHOOK, onsubmit: vi.fn(), submitLabel: 'Save connection' });
		const url = page.getByLabelText('URL');
		const secret = page.getByLabelText('Signing secret');
		await expect.element(url).toHaveAttribute('type', 'url');
		await expect.element(url).toHaveAttribute('required');
		await expect.element(secret).toHaveAttribute('type', 'password');
		await expect.element(page.getByText('Signs each request.')).toBeInTheDocument();
	});

	it('submits settings and credentials separately', async () => {
		const onsubmit = vi.fn();
		render(ConnectionForm, { descriptor: WEBHOOK, onsubmit, submitLabel: 'Save connection' });
		await page.getByLabelText('Name').fill('ops hook');
		await page.getByLabelText('URL').fill('https://example.com/hook');
		await page.getByLabelText('Signing secret').fill('s3cret');
		await page.getByRole('button', { name: 'Save connection' }).click();
		expect(onsubmit).toHaveBeenCalledWith({
			name: 'ops hook',
			settings: { url: 'https://example.com/hook' },
			credentials: { secret: 's3cret' }
		});
	});

	it('leaves credentials out when editing and no secret was typed', async () => {
		const onsubmit = vi.fn();
		render(ConnectionForm, {
			descriptor: WEBHOOK,
			onsubmit,
			submitLabel: 'Save changes',
			initial: { name: 'ops hook', settings: { url: 'https://example.com/hook' } }
		});
		await expect.element(page.getByLabelText('URL')).toHaveValue('https://example.com/hook');
		await expect.element(page.getByText('Leave empty to keep the stored value.')).toBeInTheDocument();
		await page.getByRole('button', { name: 'Save changes' }).click();
		expect(onsubmit).toHaveBeenCalledWith({ name: 'ops hook', settings: { url: 'https://example.com/hook' } });
	});
});

describe('LoginForm', () => {
	it('signs in and reports it', async () => {
		apiMock.mockResolvedValue({ id: 'u1', email: 'kris@example.com' });
		const onsignedin = vi.fn();
		render(LoginForm, { onsignedin });
		await page.getByLabelText('Email').fill('kris@example.com');
		await page.getByLabelText('Password').fill('correct horse battery');
		await page.getByRole('button', { name: 'Sign in' }).click();
		await vi.waitFor(() => expect(onsignedin).toHaveBeenCalled());
		expect(apiMock).toHaveBeenCalledWith('POST', '/api/auth/login', {
			email: 'kris@example.com',
			password: 'correct horse battery'
		});
	});

	it("shows the server's message when sign-in is refused", async () => {
		apiMock.mockRejectedValue(new ApiError(401, 'unauthorized', 'wrong email or password'));
		const onsignedin = vi.fn();
		render(LoginForm, { onsignedin });
		await page.getByLabelText('Email').fill('kris@example.com');
		await page.getByLabelText('Password').fill('nope nope nope');
		await page.getByRole('button', { name: 'Sign in' }).click();
		await expect.element(page.getByRole('alert')).toHaveTextContent('wrong email or password');
		expect(onsignedin).not.toHaveBeenCalled();
	});
});

describe('SetupFlow', () => {
	it('keeps later steps locked until the admin exists', async () => {
		render(SetupFlow, { origin: 'http://localhost:7269', onadmin: vi.fn(), started: false });
		await expect.element(page.getByRole('button', { name: 'Create account' })).toBeEnabled();
		await expect.element(page.getByRole('button', { name: 'Create source' })).toBeDisabled();
	});

	it('shows a refused account next to the form and stays on step one', async () => {
		apiMock.mockRejectedValue(new ApiError(422, 'invalid', 'the password must be at least 10 characters'));
		render(SetupFlow, { origin: 'http://localhost:7269', onadmin: vi.fn(), started: false });
		await page.getByLabelText('Email').fill('kris@example.com');
		await page.getByLabelText('Password').fill('short');
		await page.getByRole('button', { name: 'Create account' }).click();
		await expect.element(page.getByRole('alert')).toHaveTextContent('the password must be at least 10 characters');
		await expect.element(page.getByRole('button', { name: 'Create source' })).toBeDisabled();
	});

	it('unlocks the source step after the account is created, then shows the key in a curl command', async () => {
		const onadmin = vi.fn();
		apiMock.mockImplementation(async (method: string, path: string) => {
			if (path === '/api/setup') return { id: 'u1', email: 'kris@example.com' };
			if (path === '/api/connectors') return [WEBHOOK];
			if (method === 'POST' && path === '/api/sources') {
				return { id: 's1', name: 'my-service', key: 'rz_src_secretkey', key_prefix: 'rz_src_secr' };
			}
			if (path.startsWith('/api/events')) return { events: [], next_before: null };
			throw new Error(`unexpected ${method} ${path}`);
		});
		render(SetupFlow, { origin: 'http://localhost:7269', onadmin, started: false });
		await page.getByLabelText('Email').fill('kris@example.com');
		await page.getByLabelText('Password').fill('correct horse battery');
		await page.getByRole('button', { name: 'Create account' }).click();
		await vi.waitFor(() => expect(onadmin).toHaveBeenCalled());

		const createSource = page.getByRole('button', { name: 'Create source' });
		await expect.element(createSource).toBeEnabled();
		await createSource.click();
		const command = page.getByTestId('curl');
		await expect.element(command).toHaveTextContent('http://localhost:7269/v1/events');
		await expect.element(command).toHaveTextContent('Bearer rz_src_secretkey');
		await expect.element(page.getByText('Waiting for your event')).toBeInTheDocument();
	});

	it('starts at the source step when the admin already exists', async () => {
		apiMock.mockResolvedValue([WEBHOOK]);
		render(SetupFlow, { origin: 'http://localhost:7269', onadmin: vi.fn(), started: true });
		await expect.element(page.getByRole('button', { name: 'Create source' })).toBeEnabled();
		await expect.element(page.getByRole('button', { name: 'Create account' })).not.toBeInTheDocument();
	});
});
