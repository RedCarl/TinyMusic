<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type Settings, type Quality } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { refreshAppCfg } from '../lib/config.svelte';
  import { theme, setTheme, type ThemeMode } from '../lib/theme.svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';

  // ---- 更新 ----
  let appVersion = $state('');
  let updMsg = $state('');
  let updErr = $state('');
  let updBusy = $state(false);
  let pending = $state<Update | null>(null);
  let progress = $state(0);

  $effect(() => {
    getVersion().then((v) => (appVersion = v)).catch(() => {});
  });

  async function checkUpdate() {
    updBusy = true;
    updErr = '';
    updMsg = '';
    pending = null;
    try {
      const u = await check();
      if (u) {
        pending = u;
        updMsg = `发现新版本 ${u.version}`;
      } else {
        updMsg = '已经是最新版本';
      }
    } catch (e) {
      updErr = `检查失败：${e}`;
    } finally {
      updBusy = false;
    }
  }

  async function doUpdate() {
    if (!pending) return;
    updBusy = true;
    updErr = '';
    progress = 0;
    let total = 0;
    let got = 0;
    try {
      await pending.downloadAndInstall((ev) => {
        if (ev.event === 'Started') {
          total = ev.data.contentLength ?? 0;
        } else if (ev.event === 'Progress') {
          got += ev.data.chunkLength;
          progress = total > 0 ? Math.round((got / total) * 100) : 0;
        } else if (ev.event === 'Finished') {
          progress = 100;
        }
      });
      updMsg = '安装完成，正在重启…';
      await relaunch();
    } catch (e) {
      updErr = `更新失败：${e}`;
      updBusy = false;
    }
  }

  // ---- 设置（改动即保存） ----
  let s = $state<Settings>({
    download_dir: '',
    default_quality: 'high',
    concurrency: 2,
    s3_enabled: false,
    s3_endpoint: '',
    s3_region: 'us-east-1',
    s3_bucket: '',
    s3_access_key: '',
    s3_secret_key: '',
    s3_prefix: 'TinyMusic',
    s3_path_style: false,
    close_to_tray: true,
  });
  let loaded = $state(false);
  let err = $state('');
  let savedHint = $state(false);
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  onMount(async () => {
    try {
      const v = await api.settingsGet();
      // 旧配置里可能还留着已被移除的 lossless，直接替用户改成超高并存回去
      const hadLossless = v.default_quality === 'lossless';
      if (hadLossless) v.default_quality = 'super';
      s = v;
      loaded = true;
      localStorage.setItem('quality', v.default_quality);
      if (hadLossless) void saveNow();
    } catch (e) {
      err = String(e);
      loaded = true;
    }
  });

  async function saveNow() {
    if (!loaded) return;
    try {
      await api.settingsSet($state.snapshot(s) as Settings);
      localStorage.setItem('quality', s.default_quality);
      await refreshAppCfg();
      savedHint = true;
      setTimeout(() => (savedHint = false), 1500);
    } catch (e) {
      err = String(e);
    }
  }

  /** 文本输入用 400ms 防抖，避免每敲一个字都写盘 */
  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveNow(), 400);
  }

  // S3 测试
  let s3Msg = $state('');
  let s3Err = $state('');
  let s3Busy = $state(false);

  async function testS3() {
    s3Busy = true;
    s3Msg = '';
    s3Err = '';
    try {
      await saveNow();
      s3Msg = await api.s3Test();
    } catch (e) {
      s3Err = String(e);
    } finally {
      s3Busy = false;
    }
  }

  const themeOptions: { v: ThemeMode; label: string }[] = [
    { v: 'auto', label: '跟随系统' },
    { v: 'light', label: '浅色' },
    { v: 'dark', label: '深色' }
  ];

  const qualities: { v: Quality; label: string }[] = [
    { v: 'high', label: '标准 128k' },
    { v: 'super', label: '高品质 320k' },
  ];
</script>

<div class="page">
  <h1>设置</h1>

  <div class="field">
    <div class="lbl">外观</div>
    <div class="radios">
      {#each themeOptions as t (t.v)}
        <label class="radio">
          <input
            type="radio"
            name="theme"
            value={t.v}
            checked={theme.mode === t.v}
            onchange={() => setTheme(t.v)}
          />
          <span>{t.label}</span>
        </label>
      {/each}
    </div>
  </div>

  <div class="field">
    <div class="lbl">下载目录</div>
    <input
      bind:value={s.download_dir}
      oninput={scheduleSave}
      spellcheck="false"
      placeholder="~/Music/TinyMusic"
    />
  </div>

  <div class="field">
    <div class="lbl">默认音质</div>
    <div class="radios">
      {#each qualities as q}
        <label class="radio">
          <input
            type="radio"
            name="q"
            value={q.v}
            bind:group={s.default_quality}
            onchange={saveNow}
          />
          <span>{q.label}</span>
        </label>
      {/each}
    </div>
  </div>

  <div class="field">
    <div class="lbl">并发下载数</div>
    <input type="number" min="1" max="6" bind:value={s.concurrency} onchange={saveNow} />
  </div>

  <div class="field">
    <div class="lbl">窗口</div>
    <label class="check">
      <input type="checkbox" bind:checked={s.close_to_tray} onchange={saveNow} />
      <span>关闭窗口时收进托盘并继续播放</span>
    </label>
  </div>

  <h2 class="sub-h">网盘（S3 兼容）</h2>

  <div class="field">
    <label class="check">
      <input type="checkbox" bind:checked={s.s3_enabled} onchange={saveNow} />
      <span>启用网盘</span>
    </label>
  </div>

  <div class="s3grid" class:off={!s.s3_enabled}>
    <div class="field">
      <div class="lbl">Endpoint</div>
      <input
        bind:value={s.s3_endpoint}
        oninput={scheduleSave}
        placeholder="https://s3.us-east-1.amazonaws.com"
        spellcheck="false"
      />
    </div>
    <div class="field">
      <div class="lbl">Region</div>
      <input bind:value={s.s3_region} oninput={scheduleSave} placeholder="us-east-1" spellcheck="false" />
    </div>
    <div class="field">
      <div class="lbl">Bucket</div>
      <input bind:value={s.s3_bucket} oninput={scheduleSave} placeholder="my-music" spellcheck="false" />
    </div>
    <div class="field">
      <div class="lbl">前缀目录</div>
      <input bind:value={s.s3_prefix} oninput={scheduleSave} placeholder="TinyMusic" spellcheck="false" />
    </div>
    <div class="field">
      <div class="lbl">Access Key</div>
      <input bind:value={s.s3_access_key} oninput={scheduleSave} spellcheck="false" autocomplete="off" />
    </div>
    <div class="field">
      <div class="lbl">Secret Key</div>
      <input
        type="password"
        bind:value={s.s3_secret_key}
        oninput={scheduleSave}
        autocomplete="off"
      />
    </div>
  </div>

  <div class="field">
    <label class="check">
      <input type="checkbox" bind:checked={s.s3_path_style} onchange={saveNow} />
      <span>使用 Path-Style 寻址</span>
    </label>
  </div>

  <div class="actions">
    <button class="mini btn-ico" onclick={testS3} disabled={s3Busy || !s.s3_enabled}>
      <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} />
      {s3Busy ? '连接中…' : '测试连接'}
    </button>
    {#if savedHint}<span class="hint">已保存</span>{/if}
    {#if s3Msg}<span class="ok small">{s3Msg}</span>{/if}
    {#if s3Err}<span class="err small">{s3Err}</span>{/if}
    {#if err}<span class="err small">{err}</span>{/if}
  </div>

  <div class="field">
    <div class="lbl">版本</div>
    <div class="updrow">
      <span class="muted">当前 v{appVersion || '—'}</span>
      <button class="mini" onclick={checkUpdate} disabled={updBusy}>
        {updBusy && !pending ? '检查中…' : '检查更新'}
      </button>
      {#if pending}
        <button class="mini primary" onclick={doUpdate} disabled={updBusy}>下载并安装</button>
      {/if}
    </div>
    {#if updMsg}<div class="ok small">{updMsg}</div>{/if}
    {#if progress > 0 && progress < 100}
      <div class="updbar"><span style="width:{progress}%"></span></div>
    {/if}
    {#if updErr}<div class="err small">{updErr}</div>{/if}
  </div>
</div>
