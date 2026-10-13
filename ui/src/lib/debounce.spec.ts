import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { debounced } from './debounce';

describe('debounced', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it('runs once after the calls stop', () => {
		const task = vi.fn();
		const run = debounced(task, 300);
		run();
		vi.advanceTimersByTime(200);
		run();
		vi.advanceTimersByTime(299);
		expect(task).not.toHaveBeenCalled();
		vi.advanceTimersByTime(1);
		expect(task).toHaveBeenCalledTimes(1);
	});

	it('does not run after cancel', () => {
		const task = vi.fn();
		const run = debounced(task, 300);
		run();
		run.cancel();
		vi.advanceTimersByTime(1000);
		expect(task).not.toHaveBeenCalled();
	});
});
