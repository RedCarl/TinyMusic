<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type PlaylistRef } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { account } from '../lib/account.svelte';
  import { go } from '../lib/router.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { copyText, showToast } from '../lib/toast.svelte';
  import BackLink from '../components/BackLink.svelte';
  import PlaylistsSkeleton from '../components/PlaylistsSkeleton.svelte';
  import ImportQueue from '../components/ImportQueue.svelte';
  import { enqueueImport } from '../lib/importQueue.svelte';
  import { delayedVisible } from '../lib/loading.svelte';

  let playlists = $state<PlaylistRef[]>([]);
  let plLoading = $state(true);
  let err = $state('');

  const skel = delayedVisible(() => plLoading);

  let s3On = $state(false);
  let importPl = $state<PlaylistRef | null>(null);
  let importTarget = $state('');

  async function loadPlaylists() {
    if (!account.uid) {
      plLoading = false;
      return;
    }
    plLoading = true;
    err = '';
    try {
      playlists = await api.userPlaylists(account.provider, account.uid, 100);
    } catch (e) {
      err = String(e);
      playlists = [];
    } finally {
      plLoading = false;
    }
  }

  function openImport(pl: PlaylistRef) {
    importPl = pl;
    importTarget = pl.name;
  }

  function closeImport() {
    importPl = null;
  }

  function startImport() {
    const pl = importPl;
    if (!pl) return;
    if (enqueueImport(pl, importTarget)) {
      showToast('已加入导入队列');
      importPl = null;
    }
  }

  $effect(() => {
    account.uid;
    loadPlaylists();
  });

  onMount(() => {
    api
      .settingsGet()
      .then((s) => (s3On = !!s.s3_enabled && !!s.s3_bucket))
      .catch(() => (s3On = false));
  });
</script>

<div class="page">
  <BackLink />
  <h1>账号歌单</h1>

  {#if err}<div class="err">{err}</div>{/if}

  <ImportQueue />

  {#if !account.uid}
    <div class="empty">
      <div class="big" aria-hidden="true">
        <HugeiconsIcon icon={icons.account} size={ICON_SIZE.empty} />
      </div>
      <p>还没有登录</p>
      <button class="primary btn-ico" onclick={() => go({ name: 'account' })}>去登录</button>
    </div>
  {:else if !s3On}
    <div class="acct-hint">登录只用于把平台歌单导入网盘。先到「网盘」页启用网盘配置。</div>
  {/if}

  {#if account.uid}
    {#if skel.visible}
      <PlaylistsSkeleton rows={6} withIcon={s3On} />
    {:else if plLoading}
      <!-- 等骨架屏延迟 -->
    {:else if playlists.length === 0}
      <div class="muted small">这个账号还没有歌单</div>
    {:else}
      <div class="pllist wide">
        {#each playlists as pl (pl.id)}
          <div
            class="plrow"
            role="group"
            oncontextmenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              openMenuAt(e.clientX, e.clientY, [
                {
                  label: '打开歌单',
                  icon: icons.playlist,
                  run: () =>
                    go({
                      name: 'playlist',
                      provider: pl.provider,
                      playlistId: pl.id,
                      title: pl.name,
                    }),
                },
                ...(s3On
                  ? [
                      {
                        label: '一键导入到网盘',
                        icon: icons.cloud,
                        run: () => openImport(pl),
                      },
                    ]
                  : []),
                { sep: true },
                {
                  label: '复制歌单 ID',
                  icon: icons.link,
                  run: () => copyText(pl.id, '已复制歌单 ID'),
                },
              ]);
            }}
          >
            <button
              class="plopen"
              onclick={() =>
                go({ name: 'playlist', provider: pl.provider, playlistId: pl.id, title: pl.name })}
            >
              <span class="plname">{pl.name}</span>
              <span class="plnum">{pl.track_count} 首</span>
            </button>
            {#if s3On}
              <button
                class="plicon"
                title="一键导入到网盘"
                aria-label="把歌单 {pl.name} 导入到网盘"
                onclick={() => openImport(pl)}
              >
                <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

{#if importPl}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) closeImport();
    }}
  >
    <div class="modal" role="dialog" aria-modal="true" aria-label="导入歌单到网盘">
      <div class="modal-head">
        <div>
          <h2>导入到网盘</h2>
          <p class="hint">{importPl.name} · {importPl.track_count} 首 · 逐首下载后上传</p>
        </div>
        <button class="icon" onclick={closeImport} aria-label="关闭">
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      <div class="field">
        <div class="lbl">网盘歌单（目标目录）</div>
        <input bind:value={importTarget} placeholder="目标歌单名称" spellcheck="false" />
        <p class="hint" style="margin-top:8px">
          文件会存到 <code>前缀/歌单名/</code> 下，包含内嵌封面的音频、同名 .lrc 与封面图。
        </p>
        <p class="hint" style="margin-top:6px">
          加入队列后可以直接关掉弹窗，不需要在这里等；进度和结果在页面顶部的「导入队列」里看。
        </p>
      </div>

      <div class="modal-actions">
        <button class="mini" onclick={closeImport}>关闭</button>
        <button
          class="primary btn-ico"
          onclick={startImport}
          disabled={!importTarget.trim()}
        >
          <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.inline} />
          加入导入队列
        </button>
      </div>
    </div>
  </div>
{/if}
