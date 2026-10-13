import { messageOf } from './api/client';

export interface Toast {
	id: number;
	kind: 'ok' | 'problem';
	text: string;
}

const SHOWN_MS = 5000;

/** Short confirmations and errors shown at the bottom of the screen. */
class Toasts {
	list = $state<Toast[]>([]);
	private next = 1;

	ok(text: string) {
		this.push('ok', text);
	}

	/** Shows the message of anything thrown. */
	problem(error: unknown) {
		this.push('problem', messageOf(error));
	}

	dismiss(id: number) {
		this.list = this.list.filter((toast) => toast.id !== id);
	}

	private push(kind: Toast['kind'], text: string) {
		const id = this.next++;
		this.list.push({ id, kind, text });
		setTimeout(() => this.dismiss(id), SHOWN_MS);
	}
}

export const toasts = new Toasts();
