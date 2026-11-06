/** 极简 toast：复制成功、已加入播放列表这类轻反馈 */

export const toast = $state({
  msg: '',
  visible: false,
});

let timer: ReturnType<typeof setTimeout> | null = null;

export function showToast(msg: string, ms = 1800) {
  toast.msg = msg;
  toast.visible = true;
  if (timer) clearTimeout(timer);
  timer = setTimeout(() => {
    toast.visible = false;
    timer = null;
  }, ms);
}

/** 复制文本，成功/失败都给一句反馈 */
export async function copyText(text: string, label = '已复制') {
  const ok = await writeClipboard(text);
  showToast(ok ? label : '复制失败');
  return ok;
}

async function writeClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // WebView 里 navigator.clipboard 偶尔没有权限，退回到 execCommand
    try {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      ta.setAttribute('readonly', '');
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      return ok;
    } catch {
      return false;
    }
  }
}
