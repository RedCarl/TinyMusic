<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type Settings, type Quality, type QualityCapability } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { refreshAppCfg } from '../lib/config.svelte';
  import { setPreferredQuality } from '../lib/player.svelte';
  import { theme, setTheme, type ThemeMode } from '../lib/theme.svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import ProgressBar from '../components/ProgressBar.svelte';
  import { relaunch } from '@tauri-apps/plugin-process';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';

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
    default_quality: 'super',
    settings_version: 2,
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
  let caps = $state<QualityCapability[]>([]);

  onMount(async () => {
    try {
      const v = await api.settingsGet();
      s = v;
      loaded = true;
    } catch (e) {
      err = String(e);
      loaded = true;
    }
    try {
      caps = await api.qualityCapabilities();
    } catch {
      caps = [];
    }
  });

  async function saveNow() {
    if (!loaded) return;
    try {
      await api.settingsSet($state.snapshot(s) as Settings);
      setPreferredQuality(s.default_quality);
      await refreshAppCfg();
      savedHint = true;
      setTimeout(() => (savedHint = false), 1500);
    } catch (e) {
      err = String(e);
    }
  }

  /** 调系统目录选择框；取消或非 Tauri 环境（浏览器调试）就原样不动 */
  async function pickDir() {
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        title: '选择下载目录',
        defaultPath: s.download_dir || undefined
      });
      if (typeof picked === 'string' && picked) {
        s.download_dir = picked;
        await saveNow();
      }
    } catch {
      /* 浏览器里没有原生对话框，忽略 */
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

  const DEFAULT_QUALITIES: { v: Quality; label: string }[] = [
    { v: 'standard', label: '标准 128k' },
    { v: 'super', label: '高品质 320k' },
    { v: 'lossless', label: '无损 FLAC' },
    { v: 'hires', label: 'Hi-Res' },
    { v: 'atmos', label: '杜比全景声' },
    { v: 'master', label: '母带' },
  ];

  const qualityOptions = $derived(
    caps.length
      ? caps.map((c) => ({ v: c.value, label: c.label, disabled: !c.available }))
      : DEFAULT_QUALITIES.map((c) => ({
          ...c,
          disabled: c.v === 'hires' || c.v === 'atmos' || c.v === 'master',
        }))
  );
</script>

<div class="page settings-page">
  <div class="st-head">
    <h1>设置</h1>
    {#if savedHint}<span class="st-saved">已保存</span>{/if}
  </div>

  {#if err}<div class="st-error">{err}</div>{/if}

  <section class="st-section">
    <div class="st-section-title">外观</div>
    <div class="st-card">
      <div class="st-row">
        <div class="st-label">主题</div>
        <div class="st-seg" role="group" aria-label="主题">
          {#each themeOptions as t (t.v)}
            <button
              type="button"
              class:on={theme.mode === t.v}
              aria-pressed={theme.mode === t.v}
              onclick={() => setTheme(t.v)}
            >
              {t.label}
            </button>
          {/each}
        </div>
      </div>
    </div>
  </section>

  <section class="st-section">
    <div class="st-section-title">下载</div>
    <div class="st-card">
      <div class="st-row">
        <div class="st-label">下载目录</div>
        <div class="st-control st-grow">
          <input
            class="st-input"
            bind:value={s.download_dir}
            aria-label="下载目录"
            oninput={scheduleSave}
            spellcheck="false"
            placeholder="~/Music/TinyMusic"
          />
          <button class="st-btn" onclick={pickDir} title="选择下载目录">选择…</button>
        </div>
      </div>

      <div class="st-row">
        <div class="st-label">默认音质</div>
        <select class="st-select" aria-label="默认音质" bind:value={s.default_quality} onchange={saveNow}>
          {#each qualityOptions as q (q.v)}
            <option value={q.v} disabled={q.disabled}>{q.label}</option>
          {/each}
        </select>
      </div>

      <div class="st-row">
        <div class="st-label">并发下载数</div>
        <select
          class="st-select"
          aria-label="并发下载数"
          value={s.concurrency}
          onchange={(e) => {
            s.concurrency = Number(e.currentTarget.value);
            void saveNow();
          }}
        >
          {#each [1, 2, 3, 4, 5, 6] as n (n)}
            <option value={n}>{n}</option>
          {/each}
        </select>
      </div>
    </div>
  </section>

  <section class="st-section">
    <div class="st-section-title">窗口</div>
    <div class="st-card">
      <div class="st-row">
        <div class="st-label">
          关闭窗口时收进托盘
          <span class="st-desc">继续在后台播放</span>
        </div>
        <label class="st-switch">
          <input
            type="checkbox"
            aria-label="关闭窗口时收进托盘"
            bind:checked={s.close_to_tray}
            onchange={saveNow}
          />
          <span class="st-switch-track" aria-hidden="true"></span>
        </label>
      </div>
    </div>
  </section>

  <section class="st-section">
    <div class="st-section-title">网盘</div>
    <div class="st-card" class:is-off={!s.s3_enabled}>
      <div class="st-row">
        <div class="st-label">
          启用网盘
          <span class="st-desc">S3 兼容存储 · 喜欢、歌单和下载备份</span>
        </div>
        <label class="st-switch">
          <input
            type="checkbox"
            aria-label="启用网盘"
            bind:checked={s.s3_enabled}
            onchange={saveNow}
          />
          <span class="st-switch-track" aria-hidden="true"></span>
        </label>
      </div>

      <div class="st-row">
        <div class="st-label">Endpoint</div>
        <input
          class="st-input st-input-wide"
          bind:value={s.s3_endpoint}
          aria-label="S3 Endpoint"
          oninput={scheduleSave}
          placeholder="https://s3.us-east-1.amazonaws.com"
          spellcheck="false"
        />
      </div>

      <div class="st-row">
        <div class="st-label">Region</div>
        <input
          class="st-input"
          bind:value={s.s3_region}
          aria-label="S3 Region"
          oninput={scheduleSave}
          placeholder="us-east-1"
          spellcheck="false"
        />
      </div>

      <div class="st-row">
        <div class="st-label">Bucket</div>
        <input
          class="st-input"
          bind:value={s.s3_bucket}
          aria-label="S3 Bucket"
          oninput={scheduleSave}
          placeholder="my-music"
          spellcheck="false"
        />
      </div>

      <div class="st-row">
        <div class="st-label">前缀目录</div>
        <input
          class="st-input"
          bind:value={s.s3_prefix}
          aria-label="S3 前缀目录"
          oninput={scheduleSave}
          placeholder="TinyMusic"
          spellcheck="false"
        />
      </div>

      <div class="st-row">
        <div class="st-label">Access Key</div>
        <input
          class="st-input st-input-wide"
          bind:value={s.s3_access_key}
          aria-label="S3 Access Key"
          oninput={scheduleSave}
          spellcheck="false"
          autocomplete="off"
        />
      </div>

      <div class="st-row">
        <div class="st-label">Secret Key</div>
        <input
          class="st-input st-input-wide"
          type="password"
          bind:value={s.s3_secret_key}
          aria-label="S3 Secret Key"
          oninput={scheduleSave}
          autocomplete="off"
        />
      </div>

      <div class="st-row">
        <div class="st-label">Path-Style 寻址</div>
        <label class="st-switch">
          <input
            type="checkbox"
            aria-label="Path-Style 寻址"
            bind:checked={s.s3_path_style}
            onchange={saveNow}
          />
          <span class="st-switch-track" aria-hidden="true"></span>
        </label>
      </div>

      <div class="st-row">
        <div class="st-label">连接</div>
        <button class="st-btn" onclick={testS3} disabled={s3Busy || !s.s3_enabled}>
          {s3Busy ? '连接中…' : '测试连接'}
        </button>
      </div>

      {#if s3Msg || s3Err}
        <div class="st-status" class:bad={!!s3Err}>
          {s3Err || s3Msg}
        </div>
      {/if}
    </div>
  </section>

  <section class="st-section">
    <div class="st-section-title">关于</div>
    <div class="st-card">
      <div class="st-row">
        <div class="st-label">当前版本</div>
        <span class="st-value">v{appVersion || '—'}</span>
      </div>
      <div class="st-row">
        <div class="st-label">
          软件更新
          {#if updMsg}<span class="st-desc">{updMsg}</span>{/if}
        </div>
        <div class="st-control">
          {#if pending}
            <button class="st-btn primary" onclick={doUpdate} disabled={updBusy}>下载并安装</button>
          {/if}
          <button class="st-btn" onclick={checkUpdate} disabled={updBusy}>
            {updBusy && !pending ? '检查中…' : '检查更新'}
          </button>
        </div>
      </div>
      {#if updBusy && pending}
        <div class="st-status"><ProgressBar value={progress / 100} label="正在下载更新…" /></div>
      {/if}
      {#if updErr}
        <div class="st-status bad">{updErr}</div>
      {/if}
    </div>
  </section>
</div>

<style>
  .settings-page {
    max-width: 760px;
    padding-top: 26px;
  }

  .st-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 6px;
  }

  .st-head h1 {
    margin: 0;
  }

  .st-saved {
    color: var(--success);
    font-size: 12px;
  }

  .st-error {
    margin: 4px 0 12px;
    color: var(--danger);
    font-size: 12px;
  }

  .st-section {
    margin-top: 22px;
  }

  .st-section-title {
    margin: 0 6px 8px;
    color: var(--fg);
    font-size: 14px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .st-card {
    overflow: hidden;
    border: 1px solid var(--line-soft);
    border-radius: var(--r-lg);
    background-color: var(--bg2);
  }

  .st-card.is-off {
    opacity: 0.55;
  }

  .st-row {
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 48px;
    padding: 9px 16px;
  }

  .st-row + .st-row {
    border-top: 1px solid var(--line);
  }

  .st-label {
    flex: 1;
    min-width: 0;
    color: var(--fg);
    font-size: 13px;
    line-height: 1.35;
  }

  .st-desc {
    display: block;
    margin-top: 2px;
    color: var(--fg3);
    font-size: 11px;
    font-weight: 400;
  }

  .st-value {
    flex: none;
    color: var(--fg2);
    font-size: 12px;
  }

  .st-control {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .st-grow {
    flex: 1;
    min-width: 0;
  }

  .st-grow .st-input {
    flex: 1;
    min-width: 0;
  }

  .st-input {
    box-sizing: border-box;
    height: 30px;
    width: 190px;
    max-width: 100%;
    padding: 0 10px;
    border: 1px solid var(--field-edge);
    border-radius: var(--r-sm);
    background-color: var(--field);
    color: var(--fg);
    font-size: 12px;
    outline: none;
  }

  .st-input-wide {
    width: 300px;
  }

  .st-input:focus {
    border-color: var(--focus-line);
  }

  .st-select {
    box-sizing: border-box;
    height: 30px;
    min-width: 150px;
    max-width: 100%;
    padding: 0 28px 0 10px;
    border: 1px solid var(--field-edge);
    border-radius: var(--r-sm);
    background-color: var(--field);
    background-image: var(--select-arrow);
    background-repeat: no-repeat;
    background-position: right 9px center;
    color: var(--fg);
    font-size: 12px;
    appearance: none;
    outline: none;
  }

  .st-select option {
    background-color: var(--popover-solid);
    color: var(--fg);
  }

  .st-select:disabled,
  .st-select option:disabled {
    color: var(--fg3);
  }

  .st-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--field-edge);
    border-radius: var(--r-sm);
    background-color: var(--field);
    color: var(--fg);
    font-size: 12px;
    white-space: nowrap;
    cursor: pointer;
  }

  .st-btn:hover:not(:disabled) {
    background-color: var(--hover);
  }

  .st-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .st-btn.primary {
    border-color: transparent;
    background-color: #0a84ff;
    color: #fff;
  }

  .st-seg {
    display: inline-flex;
    flex: none;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--field-edge);
    border-radius: var(--r-sm);
    background-color: var(--field);
  }

  .st-seg button {
    height: 26px;
    padding: 0 12px;
    border-radius: 6px;
    color: var(--fg2);
    font-size: 12px;
    white-space: nowrap;
  }

  .st-seg button:hover {
    color: var(--fg);
  }

  .st-seg button.on {
    background-color: var(--hover);
    color: var(--fg);
    box-shadow:
      inset 0 1px 0 var(--glass-rim),
      0 1px 2px oklch(0 0 0 / 18%);
  }

  .st-switch {
    position: relative;
    flex: none;
    display: inline-flex;
    width: 40px;
    height: 24px;
    cursor: pointer;
  }

  .st-switch input {
    position: absolute;
    inset: 0;
    z-index: 1;
    width: 100%;
    height: 100%;
    margin: 0;
    opacity: 0;
    cursor: pointer;
  }

  .st-switch-track {
    position: absolute;
    inset: 0;
    border-radius: 999px;
    background-color: var(--track);
    box-shadow: inset 0 0 0 1px var(--line-strong);
    transition: background-color 160ms ease;
  }

  .st-switch-track::after {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background-color: var(--knob);
    box-shadow: 0 1px 3px oklch(0 0 0 / 28%);
    transition: transform 160ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .st-switch input:checked + .st-switch-track {
    background-color: #0a84ff;
    box-shadow: inset 0 0 0 1px oklch(0 0 0 / 8%);
  }

  .st-switch input:checked + .st-switch-track::after {
    transform: translateX(16px);
  }

  .st-switch input:focus-visible + .st-switch-track {
    outline: 2px solid var(--focus-line);
    outline-offset: 2px;
  }

  .st-status {
    padding: 8px 16px 10px;
    border-top: 1px solid var(--line);
    color: var(--fg2);
    font-size: 11px;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .st-status.bad {
    color: var(--danger);
  }

  @media (max-width: 620px) {
    .st-row {
      align-items: flex-start;
      flex-direction: column;
      gap: 8px;
    }

    .st-control,
    .st-input,
    .st-input-wide {
      width: 100%;
    }

    .st-grow {
      width: 100%;
    }
  }
</style>
