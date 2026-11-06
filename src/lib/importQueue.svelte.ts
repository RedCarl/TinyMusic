import { api, type PlaylistRef } from './api';
import { showToast } from './toast.svelte';

export type ImportStatus = 'queued' | 'running' | 'done' | 'error';

export interface ImportTask {
  id: string;
  provider: string;
  playlistId: string;
  name: string;
  target: string;
  status: ImportStatus;
  /** 0..1；null 表示还没收到进度 */
  pct: number | null;
  label: string;
  uploaded: number;
  failed: number;
  error: string;
}

/**
 * 账号歌单导入队列。
 *
 * 导入是逐首取流 → 下载 → 上传，整张歌单可能要几分钟。
 * 这里把任务排队后台跑，用户加完就能关掉弹窗，进度在账号歌单页的队列里看。
 * 同一时间只跑一个任务，避免并发打爆 CDN / S3。
 */
export const importQueue = $state({
  tasks: [] as ImportTask[],
  running: false,
});

let seq = 0;

/** 加入队列；同一来源歌单 + 目标目录已在队列里时不重复添加 */
export function enqueueImport(pl: PlaylistRef, target: string): boolean {
  const dest = target.trim() || pl.name;
  const dup = importQueue.tasks.some(
    (t) =>
      (t.status === 'queued' || t.status === 'running') &&
      t.provider === pl.provider &&
      t.playlistId === pl.id &&
      t.target === dest
  );
  if (dup) {
    showToast('这个歌单已经在导入队列里');
    return false;
  }

  importQueue.tasks.push({
    id: `${Date.now()}-${++seq}`,
    provider: pl.provider,
    playlistId: pl.id,
    name: pl.name,
    target: dest,
    status: 'queued',
    pct: null,
    label: '',
    uploaded: 0,
    failed: 0,
    error: '',
  });
  void pumpQueue();
  return true;
}

async function pumpQueue() {
  if (importQueue.running) return;
  const task = importQueue.tasks.find((t) => t.status === 'queued');
  if (!task) return;

  importQueue.running = true;
  task.status = 'running';
  task.pct = null;
  task.label = '正在导入…';
  task.error = '';

  try {
    const res = await api.s3ImportPlaylist(
      task.provider,
      task.playlistId,
      task.name,
      task.target,
      (p) => {
        if (p.total && p.task_id.startsWith('batch-') && !p.task_id.startsWith('batch-done')) {
          task.label = `第 ${p.downloaded + 1} / ${p.total} 首`;
          task.pct = Math.min(1, p.downloaded / p.total);
        }
      }
    );
    task.target = res.playlist;
    task.uploaded = res.uploaded;
    task.failed = res.failed;
    task.pct = 1;
    task.label = '完成';
    task.status = 'done';
    showToast(
      `「${task.name}」已导入 ${res.uploaded} 首到网盘` + (res.failed ? `，${res.failed} 首失败` : '')
    );
  } catch (e) {
    task.error = String(e);
    task.label = '';
    task.status = 'error';
    showToast(`「${task.name}」导入失败`);
  } finally {
    importQueue.running = false;
    void pumpQueue();
  }
}

/** 移除排队中/已结束的任务；正在跑的任务不能取消 */
export function removeImport(id: string) {
  importQueue.tasks = importQueue.tasks.filter(
    (t) => t.id !== id || t.status === 'running'
  );
}

/** 清掉已完成/失败的记录，保留排队中和进行中的 */
export function clearFinishedImports() {
  importQueue.tasks = importQueue.tasks.filter(
    (t) => t.status === 'queued' || t.status === 'running'
  );
}
