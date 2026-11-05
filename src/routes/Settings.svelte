<script lang="ts">
  import { api, type Settings, type Quality } from '../lib/api';
  import { getVersion } from '@tauri-apps/api/app';
  import { check, type Update } from '@tauri-apps/plugin-updater';
  import { relaunch } from '@tauri-apps/plugin-process';

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

  let s = $state<Settings>({ download_dir: '', default_quality: 'high', concurrency: 2 });
  let saved = $state('');
  let err = $state('');

  $effect(() => {
    api
      .settingsGet()
      .then((v) => (s = v))
      .catch((e) => (err = String(e)));
  });

  async function save() {
    try {
      await api.settingsSet($state.snapshot(s) as Settings);
      localStorage.setItem('quality', s.default_quality);
      saved = '已保存';
      setTimeout(() => (saved = ''), 2000);
    } catch (e) {
      err = String(e);
    }
  }

  const qualities: { v: Quality; label: string; note: string }[] = [
    { v: 'high', label: '标准 128k', note: '未登录唯一稳定可用的档位' },
    { v: 'super', label: '高品质 320k', note: '多数曲目会被服务端降级/拒绝' },
    { v: 'lossless', label: '无损', note: '基本不可用，仅备用' },
  ];
</script>

<div class="page">
  <h1>设置</h1>

  <div class="field">
    <div class="lbl">下载目录</div>
    <input bind:value={s.download_dir} spellcheck="false" />
  </div>

  <div class="field">
    <div class="lbl">默认音质</div>
    <div class="radios">
      {#each qualities as q}
        <label class="radio">
          <input type="radio" name="q" value={q.v} bind:group={s.default_quality} />
          <span>{q.label}</span>
          <em>{q.note}</em>
        </label>
      {/each}
    </div>
  </div>

  <div class="field">
    <div class="lbl">并发下载数</div>
    <input type="number" min="1" max="6" bind:value={s.concurrency} />
  </div>

  <div class="actions">
    <button class="primary" onclick={save}>保存</button>
    {#if saved}<span class="ok">{saved}</span>{/if}
    {#if err}<span class="err">{err}</span>{/if}
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

  <div class="note">
    <strong>关于可用性</strong>
    <p>
      本客户端不实现登录，也不做签名请求。酷我对未授权请求只下发约 11 秒的占位片段，
      因此会员专享曲目会显示「不可用」而不是卡住——这是设计上的诚实呈现，不是故障。
    </p>
  </div>
</div>
