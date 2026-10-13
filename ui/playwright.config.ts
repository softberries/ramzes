import { defineConfig } from '@playwright/test';

/** Where the ramzes binary under test listens. */
export const APP = 'http://127.0.0.1:4790';
/** Where the stand-in webhook receiver listens. */
export const RECEIVER = 'http://127.0.0.1:4791';

export default defineConfig({
	testDir: 'e2e',
	testMatch: '**/*.e2e.ts',
	// The tests share one ramzes instance and build on each other's state.
	workers: 1,
	fullyParallel: false,
	timeout: 60_000,
	use: { baseURL: APP, viewport: { width: 1440, height: 900 } },
	webServer: [
		{ command: 'node e2e/receiver.mjs 4791', url: RECEIVER, reuseExistingServer: false },
		{
			// A fresh instance on an empty data directory, serving the UI built just before.
			command:
				'npm run build && RAMZES_LISTEN=127.0.0.1:4790 RAMZES_DATA_DIR=$(mktemp -d) cargo run --manifest-path ../Cargo.toml -p ramzes-server',
			url: `${APP}/healthz`,
			reuseExistingServer: false,
			timeout: 300_000
		}
	]
});
