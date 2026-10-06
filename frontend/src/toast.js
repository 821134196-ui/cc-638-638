import { writable } from 'svelte/store';

export const toasts = writable([]);
let seq = 0;

export function pushToast(message, type = 'info', ttl = 3200) {
  const id = ++seq;
  toasts.update((list) => [...list, { id, message, type }]);
  setTimeout(() => {
    toasts.update((list) => list.filter((t) => t.id !== id));
  }, ttl);
}

export const toast = {
  info: (m) => pushToast(m, 'info'),
  success: (m) => pushToast(m, 'success'),
  error: (m) => pushToast(m, 'error', 5000),
};
