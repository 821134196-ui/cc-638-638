import { writable } from 'svelte/store';

export const user = writable(null);
export const toasts = writable([]);

let toastId = 0;
export function toast(message, kind = 'info', ttl = 4000) {
  const id = ++toastId;
  toasts.update((t) => [...t, { id, message, kind }]);
  setTimeout(() => toasts.update((t) => t.filter((x) => x.id !== id)), ttl);
}

async function request(method, path, body) {
  const opts = {
    method,
    headers: {},
    credentials: 'same-origin',
  };
  if (body !== undefined) {
    opts.headers['Content-Type'] = 'application/json';
    opts.body = JSON.stringify(body);
  }
  const resp = await fetch(`/api${path}`, opts);
  let data = null;
  const text = await resp.text();
  if (text) {
    try { data = JSON.parse(text); } catch { data = { raw: text }; }
  }
  if (!resp.ok) {
    const msg = data?.error || `请求失败 (${resp.status})`;
    const err = new Error(msg);
    err.status = resp.status;
    throw err;
  }
  return data;
}

export const api = {
  get: (p) => request('GET', p),
  post: (p, b = {}) => request('POST', p, b),
  patch: (p, b) => request('PATCH', p, b),
};

export const STATUS_LABEL = {
  in_use: { text: '使用中', cls: 'info' },
  available: { text: '空闲可分配', cls: 'ok' },
  damaged: { text: '损坏', cls: 'bad' },
  pending: { text: '待交出方确认', cls: 'warn' },
  released: { text: '待接收方确认', cls: 'purple' },
  confirmed: { text: '已完成', cls: 'ok' },
  rejected: { text: '已拒绝', cls: 'gray' },
  cancelled: { text: '已作废', cls: 'gray' },
};

export const KIND_LABEL = {
  too_large: '桌椅偏大（过高）',
  too_small: '桌椅偏小（过矮）',
  unassigned: '暂无桌椅',
};

export async function loadMe() {
  try {
    const me = await api.get('/auth/me');
    user.set(me);
    return me;
  } catch {
    user.set(null);
    return null;
  }
}
