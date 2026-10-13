export interface DiffLine {
	kind: 'same' | 'added' | 'removed';
	text: string;
}

/** Stands for a run of unchanged lines that is not shown. */
export interface DiffGap {
	kind: 'gap';
	count: number;
}

function linesOf(text: string): string[] {
	const lines = text.replace(/\r\n/g, '\n').split('\n');
	if (lines[lines.length - 1] === '') lines.pop();
	return lines;
}

/**
 * What changed between two texts, line by line: the lines of `after` in order, with the lines only
 * `before` had placed where they were. A changed line appears as removed, then added.
 */
export function lineDiff(before: string, after: string): DiffLine[] {
	const a = linesOf(before);
	const b = linesOf(after);
	const width = b.length + 1;
	// common[i * width + j] is the length of the longest common run of lines of a[i..] and b[j..].
	const common = new Uint32Array((a.length + 1) * width);
	for (let i = a.length - 1; i >= 0; i--) {
		for (let j = b.length - 1; j >= 0; j--) {
			common[i * width + j] =
				a[i] === b[j]
					? common[(i + 1) * width + j + 1] + 1
					: Math.max(common[(i + 1) * width + j], common[i * width + j + 1]);
		}
	}

	const lines: DiffLine[] = [];
	let i = 0;
	let j = 0;
	while (i < a.length && j < b.length) {
		if (a[i] === b[j]) {
			lines.push({ kind: 'same', text: a[i] });
			i++;
			j++;
		} else if (common[(i + 1) * width + j] >= common[i * width + j + 1]) {
			lines.push({ kind: 'removed', text: a[i++] });
		} else {
			lines.push({ kind: 'added', text: b[j++] });
		}
	}
	while (i < a.length) lines.push({ kind: 'removed', text: a[i++] });
	while (j < b.length) lines.push({ kind: 'added', text: b[j++] });
	return lines;
}

/**
 * A diff cut down to its changes and the `context` unchanged lines on each side of them. Each run
 * that is left out becomes one gap with its length. Empty when nothing changed.
 */
export function nearChanges(lines: DiffLine[], context: number): (DiffLine | DiffGap)[] {
	const changed = lines.flatMap((line, index) => (line.kind === 'same' ? [] : [index]));
	const shown: (DiffLine | DiffGap)[] = [];
	if (changed.length === 0) return shown;
	let next = 0;
	let hidden = 0;
	lines.forEach((line, index) => {
		// The nearest change at or after this line, and the one before it.
		while (next < changed.length && changed[next] < index) next++;
		const ahead = next < changed.length ? changed[next] - index : Infinity;
		const behind = next > 0 ? index - changed[next - 1] : Infinity;
		if (Math.min(ahead, behind) <= context) {
			if (hidden > 0) shown.push({ kind: 'gap', count: hidden });
			hidden = 0;
			shown.push(line);
		} else {
			hidden++;
		}
	});
	if (hidden > 0) shown.push({ kind: 'gap', count: hidden });
	return shown;
}
