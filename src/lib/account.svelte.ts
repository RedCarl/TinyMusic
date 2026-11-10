/**
 * 账号状态（跨页面共享）。
 *
 * - 酷我：只需要一个 uid —— 歌单 / 喜欢都只用 uid 就能读，密码、token 一概不保存。
 * - 网易云：需要登录态（cookie）才能读每日推荐/我的歌单，真正的凭据由后端
 *   `sessions` 表保管；这里只缓存用于展示的昵称/uid，避免页面闪烁。
 *
 * 两边的 uid/昵称都只是「显示用缓存」，权威状态永远来自 `session_get`。
 */

const KW_KEY = 'kuwo_uid';
const NE_KEY = 'netease_profile';

function read(key: string): string {
  try {
    return localStorage.getItem(key) ?? '';
  } catch {
    return '';
  }
}

function readProfile(): { uid: string; nickname: string; avatar: string } {
  try {
    const raw = localStorage.getItem(NE_KEY);
    if (!raw) return { uid: '', nickname: '', avatar: '' };
    const v = JSON.parse(raw) as Partial<{ uid: string; nickname: string; avatar: string }>;
    return {
      uid: v.uid ?? '',
      nickname: v.nickname ?? '',
      avatar: v.avatar ?? ''
    };
  } catch {
    return { uid: '', nickname: '', avatar: '' };
  }
}

export const account = $state({
  provider: 'kuwo',
  uid: read(KW_KEY),
  netease: readProfile()
});

export function setUid(uid: string) {
  const v = uid.trim();
  account.uid = v;
  try {
    if (v) localStorage.setItem(KW_KEY, v);
    else localStorage.removeItem(KW_KEY);
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

/** 网易云登录成功后调用：写入展示缓存 */
export function setNetease(p: { uid: string; nickname: string; avatar?: string | null }) {
  const v = { uid: p.uid, nickname: p.nickname, avatar: p.avatar ?? '' };
  account.netease = v;
  try {
    localStorage.setItem(NE_KEY, JSON.stringify(v));
  } catch {
    /* ignore */
  }
}

export function clearNetease() {
  account.netease = { uid: '', nickname: '', avatar: '' };
  try {
    localStorage.removeItem(NE_KEY);
  } catch {
    /* ignore */
  }
}

export function hasNetease(): boolean {
  return account.netease.uid.length > 0;
}
