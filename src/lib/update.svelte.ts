import { check, type Update } from '@tauri-apps/plugin-updater';

/**
 * 后台静默检查的结果。
 *
 * 这里只记「有没有新版」和可下载的句柄，不弹窗、不下载、不拦启动 —— 侧边栏
 * 「设置」那一项挂一颗红点；用户点进设置就能直接「下载并安装」，不必再点一次检查。
 */
export const updateState = $state<{ version: string }>({ version: '' });

/** plugin 的 Update 实例会被 $state 包成代理，方法里的 this 可能失真，单独存着 */
let handle: Update | null = null;

let inflight = false;

/** 当前待安装的更新句柄；没有新版时是 null */
export function pendingUpdate(): Update | null {
  return handle;
}

/** 检查结果统一从这里落地：红点和设置页的按钮都依赖 updateState.version */
export function setPendingUpdate(u: Update | null) {
  handle = u;
  updateState.version = u?.version ?? '';
}

/** 安静地查一次；离线、超时、服务端没起来都当作没这回事 */
export async function checkUpdatesSilently() {
  if (inflight) return;
  inflight = true;
  try {
    setPendingUpdate(await check());
  } catch {
    /* 静默：下一次轮询再说 */
  } finally {
    inflight = false;
  }
}

/** 用户手动检查过、确认已经是最新时，把红点撤掉 */
export function clearUpdateBadge() {
  setPendingUpdate(null);
}
