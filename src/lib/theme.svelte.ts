import { api } from './api';

export type ThemeMode = 'auto' | 'light' | 'dark';
export type ResolvedTheme = 'light' | 'dark';

const STORAGE_KEY = 'tinymusic.theme';

function storedMode(): ThemeMode {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === 'light' || v === 'dark' || v === 'auto') return v;
  } catch {
    /* ignore */
  }
  return 'auto';
}

const media = window.matchMedia('(prefers-color-scheme: dark)');

export const theme = $state<{ mode: ThemeMode; resolved: ResolvedTheme }>({
  mode: storedMode(),
  resolved: media.matches ? 'dark' : 'light'
});

function resolve(): ResolvedTheme {
  if (theme.mode === 'auto') return media.matches ? 'dark' : 'light';
  return theme.mode;
}

function apply() {
  theme.resolved = resolve();
  document.documentElement.dataset.theme = theme.resolved;
  // 窗口标题栏/原生控件跟着一起变（auto 时交还给系统）
  api.setWindowTheme(theme.mode).catch(() => {});
}

export function setTheme(mode: ThemeMode) {
  theme.mode = mode;
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    /* ignore */
  }
  apply();
}

/** 在挂载前调用，避免首帧闪一下深色 */
export function initTheme() {
  media.addEventListener('change', () => {
    if (theme.mode === 'auto') apply();
  });
  apply();
}
