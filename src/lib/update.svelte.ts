import { check } from '@tauri-apps/plugin-updater';

/**
 * 后台静默检查的结果。
 *
 * 这里只记「有没有新版」，不弹窗、不下载、不拦启动 —— 界面上只在
 * 侧边栏「设置」那一项挂一颗红点，用户点进设置再决定要不要更新。
 */
export const updateState = $state<{ version: string }>({ version: '' });

let inflight = false;

/** 安静地查一次；离线、超时、服务端没起来都当作没这回事 */
export async function checkUpdatesSilently() {
  if (inflight) return;
  inflight = true;
  try {
    const u = await check();
    updateState.version = u?.version ?? '';
  } catch {
    /* 静默：下一次轮询再说 */
  } finally {
    inflight = false;
  }
}

/** 用户手动检查过、确认已经是最新时，把红点撤掉 */
export function clearUpdateBadge() {
  updateState.version = '';
}
