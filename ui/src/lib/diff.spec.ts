import { describe, expect, it } from 'vitest';
import { lineDiff, nearChanges, type DiffLine } from './diff';

/** A diff as one string per line: `+` added, `-` removed, a space for unchanged. */
function marks(lines: DiffLine[]): string[] {
	return lines.map((line) => `${{ added: '+', removed: '-', same: ' ' }[line.kind]}${line.text}`);
}

describe('lineDiff', () => {
	it('marks nothing when the texts are the same', () => {
		expect(marks(lineDiff('a\nb', 'a\nb'))).toEqual([' a', ' b']);
	});

	it('marks added and removed lines around what stayed', () => {
		expect(marks(lineDiff('a\nb\nc', 'a\nc\nd'))).toEqual([' a', '-b', ' c', '+d']);
	});

	it('shows a changed line as removed then added', () => {
		expect(marks(lineDiff('name: ops\nstop: false', 'name: ops\nstop: true'))).toEqual([
			' name: ops',
			'-stop: false',
			'+stop: true'
		]);
	});

	it('treats an empty text as no lines', () => {
		expect(marks(lineDiff('', 'a\nb'))).toEqual(['+a', '+b']);
		expect(marks(lineDiff('a\nb', ''))).toEqual(['-a', '-b']);
		expect(lineDiff('', '')).toEqual([]);
	});

	it('ignores a newline at the end and Windows line endings', () => {
		expect(marks(lineDiff('a\nb\n', 'a\r\nb'))).toEqual([' a', ' b']);
	});

	it('keeps repeated lines in order', () => {
		expect(marks(lineDiff('- x\n- x', '- x\n- y\n- x'))).toEqual([' - x', '+- y', ' - x']);
	});
});

describe('nearChanges', () => {
	const diff = lineDiff('1\n2\n3\n4\n5\n6\n7\n8\n9', '1\n2\n3\n4\nfive\n6\n7\n8\n9');

	it('keeps the lines around each change and counts what it leaves out', () => {
		expect(nearChanges(diff, 1)).toEqual([
			{ kind: 'gap', count: 3 },
			{ kind: 'same', text: '4' },
			{ kind: 'removed', text: '5' },
			{ kind: 'added', text: 'five' },
			{ kind: 'same', text: '6' },
			{ kind: 'gap', count: 3 }
		]);
	});

	it('leaves nothing out when everything is near a change', () => {
		expect(nearChanges(diff, 4)).toEqual(diff);
	});

	it('is empty when nothing changed', () => {
		expect(nearChanges(lineDiff('a\nb', 'a\nb'), 2)).toEqual([]);
	});
});
