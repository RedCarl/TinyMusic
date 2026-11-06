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
  | { name: 'downloads' }
  | { name: 'cloud' }
  | { name: 'library' }
  | { name: 'settings' };

export const router = $state<{ route: Route }>({ route: { name: 'search' } });

export function go(r: Route) {
  router.route = r;
  scrollMainTop();
}

/** 把主滚动区域拉回顶部（切页、换搜索结果时都需要） */
export function scrollMainTop() {
  requestAnimationFrame(() => {
    document.querySelector('main')?.scrollTo({ top: 0, behavior: 'auto' });
  });
}
