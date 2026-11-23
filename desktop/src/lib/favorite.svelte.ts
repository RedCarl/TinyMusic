/**
 * 「喜欢」状态的唯一来源。
 *
 * 红心散落在曲目行、播放栏、播放列表、网盘页、歌曲详情页，各自查一次接口既慢又会不同步；
 * 这里统一缓存 provider+id 的收藏状态，任何一处点红心，其它地方跟着亮。
 *
 * 后端 favorite_check 已经处理了「清单里记的是别的歌单那份 key」的情况
 * （按歌名+歌手再认一遍），所以这里只按 provider+id 缓存，写入仍以后端为准。
 */
import { api, type Track } from './api';
import { t } from './i18n.svelte';
import { showToast } from './toast.svelte';

export const favoriteUi = $state({
  /** provider:id → 是否已收藏 */
  known: {} as Record<string, boolean>,
});

const keyOf = (track: Track) => `${track.provider}:${track.id}`;

/** 和 core 的 favorite_key 同样归一：小写、只留字母和数字 */
function dedupeOf(track: Track): string {
  const norm = (value: string) =>
    (value || '').toLowerCase().replace(/[^\p{L}\p{N}]/gu, '');
  return `${norm(track.title)}|${norm(track.artist)}`;
}

/** 是否已收藏（没查过时按未收藏显示，查完会自动重绘） */
export function isFavorite(track: Track): boolean {
  return favoriteUi.known[keyOf(track)] ?? false;
}

export function isFavoriteKnown(track: Track): boolean {
  return keyOf(track) in favoriteUi.known;
}

/** 查一次收藏状态；已经查过就不再打接口 */
export async function ensureFavorite(track: Track): Promise<void> {
  if (isFavoriteKnown(track)) return;
  try {
    favoriteUi.known[keyOf(track)] = await api.favoriteCheck(track);
  } catch {
    /* 查不到就按未收藏处理 */
  }
}

/**
 * 一次把一整页曲目的收藏状态标好：只取一次收藏清单，避免逐行 invoke。
 * 已经查过的曲目保持原状（可能是刚点过的乐观值）。
 */
export async function primeFavorites(tracks: Track[]): Promise<void> {
  if (tracks.length === 0) return;
  let list: Track[] = [];
  try {
    list = await api.favoriteList();
  } catch {
    return;
  }
  const ids = new Set(list.map(keyOf));
  const names = new Set(list.map(dedupeOf));
  for (const track of tracks) {
    if (isFavoriteKnown(track)) continue;
    favoriteUi.known[keyOf(track)] = ids.has(keyOf(track)) || names.has(dedupeOf(track));
  }
}

/**
 * 点红心：先按乐观值亮/灭，失败回滚并把错误抛给调用方显示。
 * 后端 `favorite_add` 本身也是先落本地再后台写清单，这里不额外等一下网络。
 */
export async function toggleFavorite(track: Track): Promise<boolean> {
  const before = isFavorite(track);
  const next = !before;
  favoriteUi.known[keyOf(track)] = next;
  try {
    if (next) {
      await api.favoriteAdd(track);
      showToast(t('已收藏到网盘'));
    } else {
      await api.favoriteRemove(track);
      showToast(t('已取消喜欢'));
    }
  } catch (e) {
    favoriteUi.known[keyOf(track)] = before;
    throw e;
  }
  return next;
}
