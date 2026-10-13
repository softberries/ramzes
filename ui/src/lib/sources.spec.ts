import { describe, expect, it } from 'vitest';
import { KEY_PLACEHOLDER, headerList, isBuiltIn, setupText } from './sources';

describe('setupText', () => {
	const at = { origin: 'https://ramzes.example.com', path: '/v1/in/abc' };

	it('fills in the address, the origin and the key', () => {
		expect(setupText('URL: {url}\nSecret: {key}', { ...at, key: 'rz_live_1' })).toBe(
			'URL: https://ramzes.example.com/v1/in/abc\nSecret: rz_live_1'
		);
		expect(setupText('curl {origin}/v1/events -H "Bearer {key}"', { ...at, key: 'k' })).toBe(
			'curl https://ramzes.example.com/v1/events -H "Bearer k"'
		);
	});

	it('fills in every occurrence', () => {
		expect(setupText('{url} {url} {key} {key}', { ...at, key: 'k' })).toBe(
			'https://ramzes.example.com/v1/in/abc https://ramzes.example.com/v1/in/abc k k'
		);
	});

	it('uses a placeholder when the key is not known', () => {
		expect(setupText('Secret: {key}', at)).toBe(`Secret: ${KEY_PLACEHOLDER}`);
		expect(KEY_PLACEHOLDER).toBe('<the source key>');
	});

	it('leaves other braces and a key that looks like a placeholder alone', () => {
		expect(setupText('-d \'{"topic": "a"}\' {key}', { ...at, key: '{url}' })).toBe('-d \'{"topic": "a"}\' {url}');
	});

	it('trims the blank lines around the text', () => {
		expect(setupText('\nURL: {url}\n\n', at)).toBe('URL: https://ramzes.example.com/v1/in/abc');
	});
});

describe('headerList', () => {
	it('reads stored headers as name and value pairs', () => {
		expect(headerList('{"content-type":"application/json","x-kind":"created"}')).toEqual([
			['content-type', 'application/json'],
			['x-kind', 'created']
		]);
	});

	it('is empty when there are none or they cannot be read', () => {
		expect(headerList(null)).toEqual([]);
		expect(headerList(undefined)).toEqual([]);
		expect(headerList('')).toEqual([]);
		expect(headerList('not json')).toEqual([]);
		expect(headerList('"text"')).toEqual([]);
		expect(headerList('[1]')).toEqual([]);
		expect(headerList('{}')).toEqual([]);
	});
});

describe('isBuiltIn', () => {
	it("is true only for the source of ramzes's own events", () => {
		expect(isBuiltIn({ kind: 'system' })).toBe(true);
		expect(isBuiltIn({ kind: 'native' })).toBe(false);
		expect(isBuiltIn({ kind: 'alertmanager' })).toBe(false);
	});
});
