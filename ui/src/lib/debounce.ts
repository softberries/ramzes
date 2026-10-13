/**
 * Runs `task` once `delayMs` have passed without another call. The returned function schedules a
 * run; its `cancel` drops a run that is waiting.
 */
export function debounced(task: () => void, delayMs: number): (() => void) & { cancel: () => void } {
	let timer: ReturnType<typeof setTimeout> | undefined;
	const schedule = () => {
		clearTimeout(timer);
		timer = setTimeout(task, delayMs);
	};
	schedule.cancel = () => clearTimeout(timer);
	return schedule;
}
