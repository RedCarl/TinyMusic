import type { Track } from './api';

/**
 * 路由表。
 *
 * 注意：判别字段叫 `name`，所以其它字段**不能**再叫 `name`——
 * artist / album 的显示名用 `title`，否则对象字面量里重复键会覆盖判别字段，
 * 跳转后 `route.name` 变成人名，路由直接失效。
 */
export type Route =
  | { name: 'search'; kw?: string }
  | { name: 'playlist'; provider: string; playlistId: string; title: string }
  | { name: 'track'; track: Track }
  | { name: 'artist'; provider: string; artistId: string; title: string }
  | { name: 'album'; provider: string; albumId: string; title: string }
  | { name: 'account' }
  | { name: 'accountplaylists' }
  | { name: 'downloads' }
  | { name: 'cloud' }
  | { name: 'library' }
  | { name: 'settings' };

/** 侧边栏一级页面：进入时清空返回栈 */
const TOP_LEVEL: ReadonlySet<Route['name']> = new Set([
  'search',
  'account',
  'downloads',
  'cloud',
  'library',
  'settings'
]);

export const router = $state<{ route: Route; history: Route[] }>({
  route: { name: 'search' },
  history: []
});

function sameRoute(a: Route, b: Route): boolean {
  if (a.name !== b.name) return false;
  if (a.name === 'artist' && b.name === 'artist') return a.artistId === b.artistId;
  if (a.name === 'album' && b.name === 'album') return a.albumId === b.albumId;
  if (a.name === 'playlist' && b.name === 'playlist') return a.playlistId === b.playlistId;
  if (a.name === 'track' && b.name === 'track') return a.track.id === b.track.id;
  return true;
}

export function go(r: Route) {
  const current = router.route;
  if (sameRoute(current, r)) return;

  if (TOP_LEVEL.has(r.name)) {
    router.history = [];
  } else {
    router.history = [...router.history, current];
  }
  router.route = r;
  scrollMainTop();
}

/** 返回上一级；没有可返回的页面时返回 false */
export function back(): boolean {
  const prev = router.history.pop();
  if (!prev) return false;
  router.route = prev;
  scrollMainTop();
  return true;
}

export function canBack(): boolean {
  return router.history.length > 0;
}

/** 把主滚动区域拉回顶部（切页、换搜索结果时都需要） */
export function scrollMainTop() {
  requestAnimationFrame(() => {
    document.querySelector('main')?.scrollTo({ top: 0, behavior: 'auto' });
  });
}
