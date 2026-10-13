import { describe, expect, it } from 'vitest';
import { curlForEvent } from './snippets';

describe('curlForEvent', () => {
	it('posts a sample event to this instance with the source key', () => {
		const curl = curlForEvent('https://ramzes.example.com', 'rz_src_abc123');
		expect(curl).toContain('curl -X POST https://ramzes.example.com/v1/events');
		expect(curl).toContain('Authorization: Bearer rz_src_abc123');
		expect(curl).toContain('"topic": "ramzes.hello"');
	});

	it('is one shell command: every line but the last ends in a continuation', () => {
		const lines = curlForEvent('http://localhost:7269', 'k').split('\n');
		expect(lines.slice(0, -1).every((line) => line.endsWith('\\'))).toBe(true);
		expect(lines.at(-1)?.endsWith('\\')).toBe(false);
	});

	it('uses a placeholder when the key is not known', () => {
		expect(curlForEvent('http://localhost:7269')).toContain('Bearer <source key>');
	});
});
