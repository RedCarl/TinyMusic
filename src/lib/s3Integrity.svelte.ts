/**
 * 网盘完整性检测：扫描桶里缺音频 / 歌词 / 封面的条目，逐个补齐。
 *
 * 检测本身不下载任何东西，只列对象；「补齐」才会走平台取歌词/封面
 * （缺音频时整首重传）。补齐后后端会重新扫描并返回最新结果。
 */
import { api, type S3IntegrityIssue } from './api';
import { showToast } from './toast.svelte';

export const integrityUi = $state({
  open: false,
  /** '' 表示检查整库，否则只检查这个歌单 */
  playlist: '',
  loading: false,
  checked: 0,
  issues: [] as S3IntegrityIssue[],
  /** 正在补齐的条目 id */
  busy: [] as string[],
  /** 补齐进度（已处理 / 总数），没有在补齐时为 0 */
  done: 0,
  total: 0,
  error: '',
});

export function integrityBusy(): boolean {
  return integrityUi.busy.length > 0;
}

export function fixableIssues(): S3IntegrityIssue[] {
  return integrityUi.issues.filter((i) => i.can_fix);
}

/** 打开弹窗并立即检查一次 */
export async function openIntegrity(playlist = '') {
  integrityUi.open = true;
  integrityUi.playlist = playlist;
  integrityUi.issues = [];
  integrityUi.checked = 0;
  integrityUi.error = '';
  await runIntegrityCheck();
}

export async function runIntegrityCheck() {
  if (integrityUi.loading || integrityBusy()) return;
  integrityUi.loading = true;
  integrityUi.error = '';
  try {
    const report = await api.s3IntegrityCheck(integrityUi.playlist || null);
    integrityUi.checked = report.checked;
    integrityUi.issues = report.issues;
  } catch (e) {
    integrityUi.error = String(e);
    integrityUi.issues = [];
  } finally {
    integrityUi.loading = false;
  }
}

/** 补齐指定条目；后端会重新扫描，结果直接覆盖当前列表 */
export async function fixIntegrity(ids: string[]) {
  const todo = ids.filter((id) =>
    integrityUi.issues.some((i) => i.id === id && i.can_fix)
  );
  if (!todo.length || integrityBusy()) return;

  integrityUi.error = '';
  integrityUi.busy = [...new Set([...integrityUi.busy, ...todo])];
  integrityUi.done = 0;
  integrityUi.total = todo.length;
  try {
    const report = await api.s3IntegrityFix(
      integrityUi.playlist || null,
      todo,
      (p) => {
        // 后端在处理每一项之前报一次：downloaded 是已处理数量
        integrityUi.done = Math.min(p.downloaded + 1, p.total ?? integrityUi.total);
      }
    );
    integrityUi.checked = report.checked;
    integrityUi.issues = report.issues;
    const left = report.issues.filter((i) => todo.includes(i.id));
    if (left.length) {
      showToast(`还有 ${left.length} 首没补齐`);
    } else {
      showToast('已补齐');
    }
  } catch (e) {
    integrityUi.error = String(e);
  } finally {
    integrityUi.busy = integrityUi.busy.filter((id) => !todo.includes(id));
    integrityUi.done = 0;
    integrityUi.total = 0;
  }
}

export function closeIntegrity() {
  if (integrityBusy()) return;
  integrityUi.open = false;
}
