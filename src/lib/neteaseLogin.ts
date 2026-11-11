/**
 * 网易云登录的唯一入口：打开网页登录窗口 → 轮询后端会话。
 *
 * 为什么要「网页登录」而不是在应用里做账号密码框：
 * 网易云对异地/新设备会要求短信或设备校验（`code=8821`），纯 HTTP 登录很容易
 * 卡在这一步。网页登录走的是官方前端，风控由官方处理；后端只负责：
 *   1. 从 webview 里读 `MUSIC_U`；
 *   2. 用官方接口验证一次 cookie 有效（`w/nuser/account/get`）；
 *   3. 存库 + 注入 provider。
 *
 * 前端因此**不需要**关心 cookie 长什么样，只等会话出现即可。
 */
import { api, type Session } from './api';
import { setNetease } from './account.svelte';

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/**
 * 登录只有一个入口：点「登录」→ 打开网页登录窗口 → 等后端会话出现。
 * 不再提供粘贴 cookie 的方式，界面保持和酷我一致。
 */
export async function loginNetease(timeoutMs = 300_000): Promise<Session> {
  await api.neteaseLoginOpen();
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    await sleep(1500);
    const s = await api.sessionGet('netease').catch(() => null);
    if (s && s.user_id) {
      setNetease({ uid: s.user_id, nickname: s.nickname, avatar: s.avatar });
      return s;
    }
  }
  throw new Error('登录超时：窗口已关闭或未完成登录');
}

/** 从后端兑现当前会话，用于刷新页面后校正本地展示缓存 */
export async function syncNeteaseSession(): Promise<Session | null> {
  const s = await api.sessionGet('netease').catch(() => null);
  if (s && s.user_id) {
    setNetease({ uid: s.user_id, nickname: s.nickname, avatar: s.avatar });
    return s;
  }
  return null;
}
