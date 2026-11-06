import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { initTheme } from './lib/theme.svelte';

/**
 * 窗口外观按系统分开处理：
 * - macOS：Tauri 用 Overlay 标题栏，红绿灯浮在侧边栏顶部，需要给内容留出 inset
 * - Windows：保留原生标题栏（右上角最小化/最大化/关闭），内容直接从窗口顶部开始
 * 浏览器里调试时按 web 处理，不留 macOS 的空白。
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

initTheme();

const app = mount(App, { target: document.getElementById('app')! });

export default app;
