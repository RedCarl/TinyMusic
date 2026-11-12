/**
 * 网盘完整性检测：扫描桶里缺音频 / 歌词 / 封面的条目，逐个补齐。
 *
 * 检测本身不下载任何东西，只列对象；「补齐」才会走平台取歌词/封面
 * （缺音频时整首重传）。补齐时后端逐条回传状态，列表能实时显示
 * 正在补哪一首、哪些成功、哪些失败、哪些还在排队。
 */
import { api, type S3IntegrityIssue } from './api';
import { showToast } from './toast.svelte';

export type FixStatus = 'waiting' | 'running' | 'done' | 'failed';

export const integrityUi = $state({
  open: false,
  /** '' 表示检查整库，否则只检查这个歌单 */
  playlist: '',
  loading: false,
  checked: 0,
  /** 仍然缺内容的条目 */
  issues: [] as S3IntegrityIssue[],
  /** 这一轮补齐的状态：id → waiting / running / done / failed */
  status: {} as Record<string, FixStatus>,
  /** 补齐失败的原因（同 id） */
  errors: {} as Record<string, string>,
  /** 这一轮已经补齐的条目，留在列表里让用户看到结果 */
  fixed: [] as S3IntegrityIssue[],
  /** 是否有补齐任务在跑 */
  runActive: false,
  runDone: 0,
  runTotal: 0,
  /** 最近一次补齐 / 重新检查结束的时间，歌单页据此刷新角标与封面 */
  finishedAt: 0,
  error: '',
});

export function integrityBusy(): boolean {
  return integrityUi.runActive;
}

export function issueStatus(id: string): FixStatus | undefined {
  return integrityUi.status[id];
}

export function issueError(id: string): string {
  return integrityUi.errors[id] ?? '';
}

export function fixableIssues(): S3IntegrityIssue[] {
  return integrityUi.issues.filter((i) => i.can_fix);
}

/** 清掉上一轮的运行态（结果、状态、失败原因） */
function resetRun() {
  integrityUi.status = {};
  integrityUi.errors = {};
  integrityUi.fixed = [];
  integrityUi.runActive = false;
  integrityUi.runDone = 0;
  integrityUi.runTotal = 0;
}

/** 打开弹窗并立即检查一次 */
export async function openIntegrity(playlist = '') {
  integrityUi.open = true;
  integrityUi.playlist = playlist;
  integrityUi.issues = [];
  integrityUi.checked = 0;
  integrityUi.error = '';
  resetRun();
  await runIntegrityCheck();
}

export async function runIntegrityCheck() {
  if (integrityUi.loading || integrityBusy()) return;
  integrityUi.loading = true;
  integrityUi.error = '';
  resetRun();
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

/** 补齐指定条目：一次任务里逐条处理，每条的状态实时更新 */
export async function fixIntegrity(ids: string[]) {
  const todo = ids.filter((id) =>
    integrityUi.issues.some((i) => i.id === id && i.can_fix)
  );
  if (!todo.length || integrityUi.runActive) return;

  integrityUi.error = '';
  integrityUi.runActive = true;
  integrityUi.runDone = 0;
  integrityUi.runTotal = todo.length;

  const status: Record<string, FixStatus> = { ...integrityUi.status };
  const errors: Record<string, string> = { ...integrityUi.errors };
  for (const id of todo) {
    status[id] = 'waiting';
    delete errors[id];
  }
  integrityUi.status = status;
  integrityUi.errors = errors;

  try {
    const report = await api.s3IntegrityFix(
      integrityUi.playlist || null,
      todo,
      (p) => {
        integrityUi.runDone = p.done;
        integrityUi.status = { ...integrityUi.status, [p.id]: p.status as FixStatus };
        if (p.status === 'failed') {
          integrityUi.errors = {
            ...integrityUi.errors,
            [p.id]: p.error || '补齐失败',
          };
        }
        if (p.status === 'done' && !integrityUi.fixed.some((f) => f.id === p.id)) {
          // 成功一条就立刻从待办里挪到「已补齐」，不用等整个任务跑完
          const done = integrityUi.issues.find((i) => i.id === p.id);
          if (done) {
            integrityUi.fixed = [...integrityUi.fixed, done];
            integrityUi.issues = integrityUi.issues.filter((i) => i.id !== p.id);
          }
        }
      }
    );

    // 以服务端重新扫描的结果为准
    integrityUi.checked = report.checked;
    const nextStatus: Record<string, FixStatus> = { ...integrityUi.status };
    const nextErrors: Record<string, string> = { ...integrityUi.errors };
    for (const issue of report.issues) {
      if (nextStatus[issue.id] === 'done') {
        // 说补好了、重扫却还在：按失败展示，别让用户以为已经好了
        nextStatus[issue.id] = 'failed';
        nextErrors[issue.id] = nextErrors[issue.id] || '补齐后重新检查仍然缺失';
      }
    }
    // 已经不在结果里的条目（补好了 / 列表变了）清掉状态
    const left = new Set(report.issues.map((i) => i.id));
    const pruned: Record<string, FixStatus> = {};
    const prunedErrors: Record<string, string> = {};
    for (const [id, st] of Object.entries(nextStatus)) {
      if (left.has(id)) pruned[id] = st;
    }
    for (const [id, msg] of Object.entries(nextErrors)) {
      if (left.has(id)) prunedErrors[id] = msg;
    }
    integrityUi.status = pruned;
    integrityUi.errors = prunedErrors;
    integrityUi.issues = report.issues;

    const leftSelected = report.issues.filter((i) => todo.includes(i.id));
    if (!leftSelected.length) {
      showToast('已全部补齐');
    } else {
      showToast(`还有 ${leftSelected.length} 首没补齐`);
    }
  } catch (e) {
    // 任务整体失败时，没轮到的条目回到「可重试」，别一直挂在等待中
    const status: Record<string, FixStatus> = { ...integrityUi.status };
    for (const id of todo) {
      if (status[id] === 'waiting') delete status[id];
    }
    integrityUi.status = status;
    integrityUi.error = String(e);
  } finally {
    integrityUi.runActive = false;
    integrityUi.runDone = 0;
    integrityUi.runTotal = 0;
    integrityUi.finishedAt = Date.now();
  }
}

export function closeIntegrity() {
  if (integrityBusy()) return;
  integrityUi.open = false;
}
