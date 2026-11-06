<script lang="ts">
  import { onMount } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type S3PlaylistInfo, type S3TrackRow } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { go } from '../lib/router.svelte';
  import { appCfg, refreshAppCfg } from '../lib/config.svelte';
  import { showToast } from '../lib/toast.svelte';
  import { player, playTrack, toggle } from '../lib/player.svelte';
  import SkeletonList from '../components/SkeletonList.svelte';

  let lists = $state<S3PlaylistInfo[]>([]);
  let rows = $state<S3TrackRow[]>([]);
  let current = $state<string | null>(null);
  let loading = $state(true);
  let detailLoading = $state(false);
  let err = $state('');

  type Dialog =
    | { kind: 'create' }
    | { kind: 'rename'; name: string }
    | { kind: 'delete'; name: string; count: number };

  // 拖拽排序：用鼠标事件自己做，比 HTML5 DnD 在 WKWebView 里稳
  let dragIndex = $state<number | null>(null);
  let suppressClick = false;
  let startY = 0;
  let dragging = $state(false);

  function onRowMouseDown(e: MouseEvent, i: number) {
    if (e.button !== 0) return;
    const el = e.target as HTMLElement;
    if (el.closest('button')) return;
    dragIndex = i;
    startY = e.clientY;
    dragging = false;
    suppressClick = false;
    window.addEventListener('mousemove', onRowMouseMove);
    window.addEventListener('mouseup', onRowMouseUp, { once: true });
  }

  function onRowMouseMove(e: MouseEvent) {
    if (dragIndex === null) return;
    if (!dragging) {
      if (Math.abs(e.clientY - startY) < 4) return;
      dragging = true;
      suppressClick = true;
    }
    const row = (document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null)?.closest(
      '.plrow'
    ) as HTMLElement | null;
    const name = row?.dataset.name;
    if (!name) return;
    const to = lists.findIndex((p) => p.name === name);
    if (to < 0 || to === dragIndex) return;
    const [item] = lists.splice(dragIndex, 1);
    lists.splice(to, 0, item);
    dragIndex = to;
  }

  function onRowMouseUp() {
    window.removeEventListener('mousemove', onRowMouseMove);
    const moved = dragging;
    dragging = false;
    dragIndex = null;
    if (!moved) return;
    dragging = false;
    void api
      .s3ReorderPlaylists(lists.map((p) => p.name))
      .catch((e) => {
        err = String(e);
        return loadList();
      })
      .finally(() => {
        setTimeout(() => (suppressClick = false), 250);
      });
  }

  let dialog = $state<Dialog | null>(null);
  let dialogValue = $state('');
  let dialogErr = $state('');
  let dialogBusy = $state(false);

  const FAVORITES = '喜欢';

  async function loadList() {
    loading = true;
    err = '';
    try {
      await refreshAppCfg();
      lists = appCfg.s3Enabled ? await api.s3Playlists() : [];
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(loadList);

  async function openPlaylist(name: string) {
    current = name;
    detailLoading = true;
    err = '';
    rows = [];
    try {
      rows = await api.s3PlaylistTracks(name);
    } catch (e) {
      err = String(e);
    } finally {
      detailLoading = false;
    }
  }

  function back() {
    current = null;
    rows = [];
    void loadList();
  }

  function tracks() {
    return rows.map((r) => r.track);
  }

  function playAll() {
    if (rows.length) void playTrack(rows[0].track, tracks());
  }

  function playRow(i: number) {
    const r = rows[i];
    if (player.current?.id === r.track.id) toggle();
    else void playTrack(r.track, tracks());
  }

  const isPlaying = (key: string) => player.current?.id === key && player.status === 'playing';

  function canManage(name: string) {
    return name !== FAVORITES;
  }

  // ---- 弹窗 ----
  function openCreate() {
    dialog = { kind: 'create' };
    dialogValue = '';
    dialogErr = '';
  }

  function openRename(name: string) {
    dialog = { kind: 'rename', name };
    dialogValue = name;
    dialogErr = '';
  }

  function openDelete(name: string) {
    const found = lists.find((p) => p.name === name);
    dialog = { kind: 'delete', name, count: found?.count ?? 0 };
    dialogErr = '';
  }

  function closeDialog() {
    if (dialogBusy) return;
    dialog = null;
    dialogErr = '';
  }

  async function submitDialog() {
    if (!dialog || dialogBusy) return;
    dialogErr = '';

    if (dialog.kind === 'create') {
      const name = dialogValue.trim();
      if (!name) return;
      dialogBusy = true;
      try {
        await api.s3PlaylistCreate(name);
        dialog = null;
        await loadList();
        showToast('已新建歌单');
      } catch (e) {
        dialogErr = String(e);
      } finally {
        dialogBusy = false;
      }
      return;
    }

    if (dialog.kind === 'rename') {
      const name = dialogValue.trim();
      if (!name || name === dialog.name) {
        dialog = null;
        return;
      }
      dialogBusy = true;
      const oldName = dialog.name;
      try {
        const next = await api.s3PlaylistRename(oldName, name);
        dialog = null;
        if (current === oldName) await openPlaylist(next);
        await loadList();
        showToast('歌单已重命名');
      } catch (e) {
        dialogErr = String(e);
      } finally {
        dialogBusy = false;
      }
      return;
    }

    dialogBusy = true;
    try {
      const name = dialog.name;
      await api.s3PlaylistDelete(name);
      dialog = null;
      if (current === name) back();
      else await loadList();
      showToast('歌单已删除');
    } catch (e) {
      dialogErr = String(e);
    } finally {
      dialogBusy = false;
    }
  }

  async function deleteTrack(r: S3TrackRow) {
    try {
      await api.s3TrackDelete(r.key);
      rows = rows.filter((x) => x.key !== r.key);
      showToast('已删除');
      void loadList();
    } catch (e) {
      err = String(e);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape' && dialog) closeDialog();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="page cloud">
  {#if current === null}
    <div class="cloud-top">
      <h1>网盘</h1>
      <div class="cloud-top-actions">
        <button class="icon-btn" title="刷新" aria-label="刷新" onclick={loadList} disabled={loading}>
          <HugeiconsIcon icon={icons.refresh} size={ICON_SIZE.inline} />
        </button>
        <button class="primary btn-ico" onclick={openCreate} disabled={!appCfg.s3Enabled}>
          <HugeiconsIcon icon={icons.plus} size={ICON_SIZE.inline} />
          新建歌单
        </button>
      </div>
    </div>

    {#if err}<div class="err">{err}</div>{/if}

    {#if loading}
      <SkeletonList rows={4} />
    {:else if !appCfg.s3Enabled}
      <div class="empty">
        <div class="big" aria-hidden="true">
          <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.empty} />
        </div>
        <p>网盘还没有启用</p>
        <button class="primary btn-ico" onclick={() => go({ name: 'settings' })}>去启用</button>
      </div>
    {:else if lists.length === 0}
      <div class="empty">
        <div class="big" aria-hidden="true">
          <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.empty} />
        </div>
        <p>还没有歌单</p>
      </div>
    {:else}
      <div class="plrows">
        {#each lists as p, i (p.name)}
          <div
            class="plrow"
            class:dragging={dragging && dragIndex === i}
            role="button"
            tabindex="0"
            data-name={p.name}
            onmousedown={(e) => onRowMouseDown(e, i)}
            onclick={() => {
              if (suppressClick) return;
              openPlaylist(p.name);
            }}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') openPlaylist(p.name);
            }}
          >
            <span class="plgrip" title="按住拖动排序" aria-hidden="true">
              <HugeiconsIcon icon={icons.grip} size={ICON_SIZE.inline} />
            </span>

            <span class="plart">
              {#if p.cover}
                <img src={p.cover} alt="" loading="lazy" />
              {:else}
                <HugeiconsIcon icon={icons.playlist} size={ICON_SIZE.control} />
              {/if}
            </span>

            <span class="pltext">
              <span class="plname">{p.name}</span>
              <span class="plsub">{p.count} 首</span>
            </span>

            <span class="placts">
              <button
                class="icon-btn"
                title="播放这个歌单"
                aria-label="播放 {p.name}"
                onclick={(e) => {
                  e.stopPropagation();
                  openPlaylist(p.name).then(() => playAll());
                }}
              >
                <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
              </button>
              {#if canManage(p.name)}
                <button
                  class="icon-btn"
                  title="重命名"
                  aria-label="重命名 {p.name}"
                  onclick={(e) => {
                    e.stopPropagation();
                    openRename(p.name);
                  }}
                >
                  <HugeiconsIcon icon={icons.edit} size={ICON_SIZE.inline} />
                </button>
                <button
                  class="icon-btn danger"
                  title="删除歌单"
                  aria-label="删除 {p.name}"
                  onclick={(e) => {
                    e.stopPropagation();
                    openDelete(p.name);
                  }}
                >
                  <HugeiconsIcon icon={icons.trash} size={ICON_SIZE.inline} />
                </button>
              {/if}
            </span>
          </div>
        {/each}
      </div>
    {/if}
  {:else}
    <div class="cloud-top">
      <button class="icon-btn" title="返回" aria-label="返回" onclick={back}>
        <HugeiconsIcon icon={icons.back} size={ICON_SIZE.inline} />
      </button>
      <h1>{current}</h1>
      <span class="muted">{rows.length} 首</span>
      <div class="cloud-top-actions">
        <button class="primary btn-ico" onclick={playAll} disabled={!rows.length}>
          <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
          播放全部
        </button>
        {#if canManage(current)}
          <button class="mini btn-ico" onclick={() => openRename(current!)}>
            <HugeiconsIcon icon={icons.edit} size={ICON_SIZE.inline} />
            重命名
          </button>
        {/if}
      </div>
    </div>

    {#if err}<div class="err">{err}</div>{/if}

    {#if detailLoading}
      <SkeletonList rows={6} />
    {:else if rows.length === 0}
      <div class="empty"><p>这个歌单还是空的</p></div>
    {:else}
      <div class="s3list">
        {#each rows as r, i (r.key)}
          <div class="s3row" class:on={player.current?.id === r.key}>
            <span class="idx">{i + 1}</span>
            {#if r.track.cover}
              <img class="s3cover" src={r.track.cover} alt="" loading="lazy" />
            {:else}
              <span class="s3cover ph">
                <HugeiconsIcon icon={icons.music} size={ICON_SIZE.inline} />
              </span>
            {/if}
            <span class="s3meta">
              <span class="s3title">{r.track.title || '未命名'}</span>
              <span class="s3sub">{r.track.artist || '未知歌手'}</span>
            </span>
            <button
              class="playbtn"
              aria-label={isPlaying(r.key) ? '暂停' : '播放'}
              onclick={() => playRow(i)}
            >
              <HugeiconsIcon
                icon={isPlaying(r.key) ? icons.pause : icons.play}
                size={ICON_SIZE.inline}
              />
            </button>
            {#if canManage(current)}
              <button
                class="icon-btn danger"
                title="删除这首歌"
                aria-label="删除 {r.track.title}"
                onclick={() => deleteTrack(r)}
              >
                <HugeiconsIcon icon={icons.trash} size={ICON_SIZE.inline} />
              </button>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>

{#if dialog}
  <div
    class="modal-mask"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) closeDialog();
    }}
  >
    <div class="modal narrow" role="dialog" aria-modal="true" aria-label="歌单操作">
      <div class="modal-head">
        <h2>
          {dialog.kind === 'create' ? '新建歌单' : dialog.kind === 'rename' ? '重命名歌单' : '删除歌单'}
        </h2>
        <button class="icon" onclick={closeDialog} aria-label="关闭" disabled={dialogBusy}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      {#if dialog.kind === 'delete'}
        <p class="modal-note">
          删除「{dialog.name}」会同时删除云端 {dialog.count} 个文件，无法恢复。
        </p>
      {:else}
        <input
          class="dialog-input"
          bind:value={dialogValue}
          placeholder="歌单名称"
          spellcheck="false"
          maxlength="60"
          onkeydown={(e) => {
            if (e.key === 'Enter') submitDialog();
          }}
        />
      {/if}

      {#if dialogErr}<div class="err small">{dialogErr}</div>{/if}

      <div class="modal-actions">
        <button class="mini" onclick={closeDialog} disabled={dialogBusy}>取消</button>
        <button
          class="mini"
          class:primary={dialog.kind !== 'delete'}
          class:danger={dialog.kind === 'delete'}
          onclick={submitDialog}
          disabled={dialogBusy || (dialog.kind !== 'delete' && !dialogValue.trim())}
        >
          {dialogBusy ? '处理中…' : dialog.kind === 'delete' ? '删除' : '确定'}
        </button>
      </div>
    </div>
  </div>
{/if}
