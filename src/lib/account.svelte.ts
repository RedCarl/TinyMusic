/**
 * 账号状态（跨页面共享）。
 *
 * 目前只需要一个 uid —— 酷我的歌单 / 喜欢都只用 uid 就能读，
 * 密码、token 一概不保存。uid 持久化在 localStorage。
 */

const KEY = 'kuwo_uid';

function read(): string {
  try {
    return localStorage.getItem(KEY) ?? '';
  } catch {
    return '';
  }
}

export const account = $state({
  provider: 'kuwo',
  uid: read(),
});

export function setUid(uid: string) {
  const v = uid.trim();
  account.uid = v;
  try {
    if (v) localStorage.setItem(KEY, v);
    else localStorage.removeItem(KEY);
  } catch {
    /* 隐私模式下忽略 */
  }
}

export function clearUid() {
  setUid('');
}

export function hasAccount(): boolean {
  return account.uid.length >= 4;
}
