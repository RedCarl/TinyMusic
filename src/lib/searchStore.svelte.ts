/**
 * 搜索页状态缓存 + 历史查询记录。
 *
 * 搜索结果是模块级 `$state`，切到别的页面再切回来时**不会重新请求**，
 * 页码、关键词、艺人专辑区、滚动位置都原样恢复。
 */
import type { AlbumRef, SearchPage } from './api';

const HISTORY_KEY = 'search_history';
const HISTORY_MAX = 20;

export interface HistoryItem {
  kw: string;
  at: number;
}

function readHistory(): HistoryItem[] {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    if (!raw) return [];
    const arr = JSON.parse(raw) as unknown;
    if (!Array.isArray(arr)) return [];
    return arr
      .filter(
        (x): x is HistoryItem =>
          !!x && typeof (x as HistoryItem).kw === 'string' && (x as HistoryItem).kw.trim() !== ''
      )
      .slice(0, HISTORY_MAX);
  } catch {
    return [];
  }
}

function persistHistory(items: HistoryItem[]) {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(items));
  } catch {
    /* 隐私模式忽略 */
  }
}

/** 搜索页的完整状态（跨路由保留） */
export const searchState = $state({
  kw: '',
  page: 0,
  result: null as SearchPage | null,
  artistName: '',
  artistId: '',
  albums: [] as AlbumRef[],
  loading: false,
  err: '',
  /** 是否真正发起过搜索（区分「初始空态」和「搜完 0 条」） */
  ran: false,
  /** 主区域滚动位置，回到页面时恢复 */
  scrollTop: 0,
});

/** 历史查询记录（最近在前） */
export const searchHistory = $state({ items: readHistory() });

export function addHistory(kw: string) {
  const q = kw.trim();
  if (!q) return;
  const rest = searchHistory.items.filter((x) => x.kw !== q);
  searchHistory.items = [{ kw: q, at: Date.now() }, ...rest].slice(0, HISTORY_MAX);
  persistHistory(searchHistory.items);
}

export function removeHistory(kw: string) {
  searchHistory.items = searchHistory.items.filter((x) => x.kw !== kw);
  persistHistory(searchHistory.items);
}

export function clearHistory() {
  searchHistory.items = [];
  persistHistory(searchHistory.items);
}

/** 回到搜索页时用：把状态原样恢复（不重新请求） */
export function hasCachedResult(): boolean {
  return !!searchState.result;
}
