/**
 * 下载失败（音频 / 歌词 / 封面）的记录与手动重试。
 *
 * 后端在每次自动重试 5 次仍失败后写一条记录，任务结束时会发
 * `download://failed` 事件；这里把它接进弹窗，用户可以只重试失败的部分。
 * 记录存在 SQLite 里，重启后还在（下载页有入口）。
 */
import { api, type FailedDownload } from './api';
import { showToast } from './toast.svelte';

export const failureUi = $state({
  open: false,
  items: [] as FailedDownload[],
  /** 正在重试的记录 id */
  busy: [] as string[],
  error: '',
});

export function anyBusy(): boolean {
  return failureUi.busy.length > 0;
}

/** 从后端重新读一遍（应用启动、打开下载页时调用） */
export async function loadFailures() {
  try {
    failureUi.items = await api.downloadFailures();
  } catch {
    /* 后端不可用时保持现状 */
  }
}

export function openFailures() {
  failureUi.open = true;
  failureUi.error = '';
  void loadFailures();
}

export function closeFailures() {
  if (anyBusy()) return;
  failureUi.open = false;
}

/** 任务结束时后端推过来的新失败记录：合并进列表并弹窗 */
export function addFailures(list: FailedDownload[]) {
  if (!list?.length) return;
  const map = new Map(failureUi.items.map((f) => [f.id, f]));
  for (const f of list) map.set(f.id, f);
  failureUi.items = [...map.values()].sort((a, b) => b.created_at - a.created_at);
  failureUi.error = '';
  failureUi.open = true;
}

/** 重试指定的记录；后端返回重试后仍然失败的完整列表 */
export async function retryFailures(ids: string[]) {
  const todo = ids.filter((id) => failureUi.items.some((f) => f.id === id));
  if (!todo.length || anyBusy()) return;

  failureUi.error = '';
  failureUi.busy = [...new Set([...failureUi.busy, ...todo])];
  try {
    failureUi.items = await api.downloadRetryFailed(todo);
    const left = failureUi.items.filter((f) => todo.includes(f.id));
    if (!left.length) {
      showToast('重试成功');
      if (!failureUi.items.length) failureUi.open = false;
    } else {
      showToast(`还有 ${left.length} 首没成功`);
    }
  } catch (e) {
    failureUi.error = String(e);
  } finally {
    failureUi.busy = failureUi.busy.filter((id) => !todo.includes(id));
  }
}

/** 忽略一条记录（不删已经下好的文件） */
export async function dismissFailure(id: string) {
  if (anyBusy()) return;
  try {
    await api.downloadFailureDismiss(id);
    failureUi.items = failureUi.items.filter((f) => f.id !== id);
    if (!failureUi.items.length) failureUi.open = false;
  } catch (e) {
    failureUi.error = String(e);
  }
}

export async function dismissAllFailures() {
  if (anyBusy()) return;
  try {
    await api.downloadFailuresClear();
    failureUi.items = [];
    failureUi.open = false;
  } catch (e) {
    failureUi.error = String(e);
  }
}
