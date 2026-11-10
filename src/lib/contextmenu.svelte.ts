/**
 * 全局右键菜单状态。
 *
 * 菜单就是「一组动作 + 一个坐标」，由 <ContextMenu /> 统一渲染，
 * 各列表只负责在 oncontextmenu 里调 openMenu / 专用 helper 拼菜单项。
 */

export interface MenuItem {
  label: string;
  icon?: any;
  /** 右侧灰色小字，例如快捷键/状态 */
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  /** 当前选中项：用按钮背景表示，不用勾 */
  selected?: boolean;
  /** 二级菜单：鼠标悬停展开（音质选择等） */
  submenu?: MenuEntry[];
  run?: () => void | Promise<void>;
}

export type MenuEntry = MenuItem | { sep: true };

export const ctxMenu = $state({
  open: false,
  x: 0,
  y: 0,
  items: [] as MenuEntry[],
});

/** 直接在屏幕坐标处打开菜单（右键事件已经 preventDefault 之后用） */
export function openMenuAt(x: number, y: number, items: MenuEntry[]) {
  if (items.length === 0) return;
  ctxMenu.items = items;
  ctxMenu.x = x;
  ctxMenu.y = y;
  ctxMenu.open = true;
}

/** 右键事件入口：顺手阻止浏览器默认菜单 */
export function openMenu(e: MouseEvent, items: MenuEntry[]) {
  e.preventDefault();
  e.stopPropagation();
  openMenuAt(e.clientX, e.clientY, items);
}

export function closeMenu() {
  ctxMenu.open = false;
}
