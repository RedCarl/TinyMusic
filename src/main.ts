import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { initTheme } from './lib/theme.svelte';

/**
 * 窗口外观按系统分开处理：
 * - macOS：Tauri 用 Overlay 标题栏，红绿灯浮在侧边栏顶部，需要给内容留出 inset
 * - Windows：关掉原生边框，顶部换成应用自绘的标题栏（品牌 + 拖动区 + 右上角窗口按钮），
 *   所以没有任何系统留白，标题栏那一行由布局自己撑出来
 * 浏览器里调试时按 web 处理，既不画标题栏，也不留 macOS 的空白。
 */
function detectShell(): { os: string; shell: 'tauri' | 'web' } {
  const tauri = '__TAURI_INTERNALS__' in window;
  const ua = navigator.userAgent;
  const os = /Macintosh|Mac OS X/i.test(ua)
    ? 'macos'
    : /Windows/i.test(ua)
      ? 'windows'
      : /Linux|X11/i.test(ua)
        ? 'linux'
        : 'other';
  return { os, shell: tauri ? 'tauri' : 'web' };
}

const { os, shell } = detectShell();
document.documentElement.dataset.os = os;
document.documentElement.dataset.shell = shell;
// 只有 Windows + Tauri 才是自绘标题栏；macOS 留着系统红绿灯，浏览器里什么都没有
document.documentElement.dataset.chrome =
  shell === 'tauri' && os === 'windows' ? 'custom' : 'native';

initTheme();

const app = mount(App, { target: document.getElementById('app')! });

export default app;
