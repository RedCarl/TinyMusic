<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { HugeiconsIcon } from '@hugeicons/svelte';
  import { api, type S3PlaylistInfo, type S3TrackRow } from '../lib/api';
  import { icons, ICON_SIZE } from '../lib/icons';
  import { go } from '../lib/router.svelte';
  import { appCfg, refreshAppCfg } from '../lib/config.svelte';
  import { showToast } from '../lib/toast.svelte';
  import { openMenuAt } from '../lib/contextmenu.svelte';
  import { player, playTrack, playNext, toggle } from '../lib/player.svelte';
  import CloudSkeleton from '../components/CloudSkeleton.svelte';
  import MorphIcon from '../components/MorphIcon.svelte';
  import { delayedVisible } from '../lib/loading.svelte';
  import { openIntegrity, integrityUi } from '../lib/s3Integrity.svelte';
  import { primeFavorites, isFavorite, toggleFavorite } from '../lib/favorite.svelte';
  import ErrorNotice from '../components/ErrorNotice.svelte';
  import { t } from '../lib/i18n.svelte';

  let lists = $state<S3PlaylistInfo[]>([]);
  let rows = $state<S3TrackRow[]>([]);
  let current = $state<string | null>(null);
  /** 当前打开的是「未分类」（前缀根目录的散歌） */
  let currentRoot = $state(false);
  let loading = $state(true);
  let detailLoading = $state(false);
  let err = $state('');

  const listSkel = delayedVisible(() => loading && appCfg.s3Enabled && lists.length === 0);
  const detailSkel = delayedVisible(() => detailLoading);

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
    if (lists[i]?.root) return; // 「未分类」固定在最后，不参与排序
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
    if (to < 0 || to === dragIndex || lists[to]?.root) return;
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
      .s3ReorderPlaylists(lists.filter((p) => !p.root).map((p) => p.name))
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

  async function openPlaylist(name: string, root = false) {
    current = name;
    currentRoot = root;
    detailLoading = true;
    err = '';
    rows = [];
    try {
      rows = await api.s3PlaylistTracks(name, root);
      // 红心状态一次标好：只查一次收藏清单，不逐行打接口
      void primeFavorites(rows.map((r) => r.track));
    } catch (e) {
      err = String(e);
    } finally {
      detailLoading = false;
    }
  }

  /** 点红心：网盘里的歌和平台歌走同一条收藏链路 */
  async function toggleFav(r: S3TrackRow) {
    try {
      const next = await toggleFavorite(r.track);
      if (!next && current === FAVORITES) {
        // 在「喜欢」列表里取消喜欢，这首歌就不属于这里了，直接拿掉这一行
        rows = rows.filter((x) => x.key !== r.key);
      }
    } catch (e) {
      err = String(e);
    }
  }

  function back() {
    current = null;
    currentRoot = false;
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

  const PART_LABEL: Record<string, string> = { lyrics: '歌词', cover: '封面', audio: '音频' };
  function partLabel(part: string): string {
    const label = PART_LABEL[part];
    return label ? t(label) : part;
  }
  function partsText(list: string[]): string {
    return list.map((p) => partLabel(p)).join(t('、'));
  }
  const missingCount = $derived(rows.filter((r) => (r.missing?.length ?? 0) > 0).length);

  /** 补齐结束后静默刷新当前歌单，让角标 / 封面立刻跟上 */
  $effect(() => {
    const at = integrityUi.finishedAt;
    if (!at) return;
    untrack(() => {
      if (current) {
        api
          .s3PlaylistTracks(current, currentRoot)
          .then((next) => (rows = next))
          .catch(() => {});
      } else {
        void loadList();
      }
    });
  });

  function canManage(name: string, root = false) {
    return name !== FAVORITES && !root;
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
        showToast(t('已新建歌单'));
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
        showToast(t('歌单已重命名'));
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
      showToast(t('歌单已删除'));
    } catch (e) {
      dialogErr = String(e);
    } finally {
      dialogBusy = false;
    }
  }

  function rowMenu(e: MouseEvent, r: S3TrackRow, i: number) {
    e.preventDefault();
    e.stopPropagation();
    openMenuAt(e.clientX, e.clientY, [
      {
        label: isPlaying(r.key) ? t('暂停') : t('播放'),
        icon: isPlaying(r.key) ? icons.pause : icons.play,
        run: () => {
          const idx = rows.findIndex((x) => x.key === r.key);
          if (idx >= 0) playRow(idx);
        },
      },
      {
        label: t('下一首播放'),
        icon: icons.next,
        run: () => {
          playNext(r.track);
          showToast(t('已设为下一首'));
        },
      },
      ...(appCfg.s3Enabled
        ? [
            {
              label: isFavorite(r.track) ? t('取消喜欢') : t('喜欢（收藏到网盘）'),
              icon: icons.favorite,
              run: () => toggleFav(r),
            },
          ]
        : []),
      ...(current && canManage(current)
        ? [
            { sep: true as const },
            {
              label: t('删除这首歌'),
              icon: icons.trash,
              danger: true,
              run: () => deleteTrack(r),
            },
          ]
        : []),
    ]);
  }

  async function deleteTrack(r: S3TrackRow) {
    try {
      await api.s3TrackDelete(r.key);
      rows = rows.filter((x) => x.key !== r.key);
      showToast(t('已删除'));
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
      <h1>{t('网盘')}</h1>
      <div class="cloud-top-actions">
        <button
          class="icon-btn"
          title={t('检查完整性')}
          aria-label={t('检查网盘完整性')}
          onclick={() => openIntegrity('')}
          disabled={!appCfg.s3Enabled}
        >
          <HugeiconsIcon icon={icons.shield} size={ICON_SIZE.inline} />
        </button>
        <button
          class="icon-btn"
          title={t('刷新')}
          aria-label={t('刷新')}
          onclick={loadList}
          disabled={loading}
        >
          <HugeiconsIcon icon={icons.refresh} size={ICON_SIZE.inline} />
        </button>
        <button class="primary btn-ico" onclick={openCreate} disabled={!appCfg.s3Enabled}>
          <HugeiconsIcon icon={icons.plus} size={ICON_SIZE.inline} />
          {t('新建歌单')}
        </button>
      </div>
    </div>

    {#if err}<ErrorNotice text={err} />{/if}

    {#if listSkel.visible}
      <CloudSkeleton mode="list" />
    {:else if !appCfg.s3Enabled && !loading}
      <div class="empty">
        <div class="big" aria-hidden="true">
          <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.empty} />
        </div>
        <p>{t('网盘还没有启用')}</p>
        <button class="primary btn-ico" onclick={() => go({ name: 'settings' })}>{t('去启用')}</button>
      </div>
    {:else if lists.length === 0 && !loading}
      <div class="empty">
        <div class="big" aria-hidden="true">
          <HugeiconsIcon icon={icons.cloud} size={ICON_SIZE.empty} />
        </div>
        <p>{t('还没有歌单')}</p>
      </div>
    {:else if lists.length > 0}
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
              openPlaylist(p.name, p.root);
            }}
            onkeydown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') openPlaylist(p.name, p.root);
            }}
          >
            <span class="plgrip" title={t('按住拖动排序')} aria-hidden="true">
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
              <span class="plsub"
                >{t('{n} 首', { n: p.count })}{#if p.root} · {t('根目录')}{/if}</span
              >
            </span>

            <span class="placts">
              <button
                class="icon-btn"
                title={t('播放这个歌单')}
                aria-label={t('播放 {name}', { name: p.name })}
                onclick={(e) => {
                  e.stopPropagation();
                  openPlaylist(p.name, p.root).then(() => playAll());
                }}
              >
                <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
              </button>
              {#if canManage(p.name, p.root)}
                <button
                  class="icon-btn"
                  title={t('重命名')}
                  aria-label={t('重命名 {name}', { name: p.name })}
                  onclick={(e) => {
                    e.stopPropagation();
                    openRename(p.name);
                  }}
                >
                  <HugeiconsIcon icon={icons.edit} size={ICON_SIZE.inline} />
                </button>
                <button
                  class="icon-btn danger"
                  title={t('删除歌单')}
                  aria-label={t('删除 {name}', { name: p.name })}
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
      <button class="icon-btn" title={t('返回')} aria-label={t('返回')} onclick={back}>
        <HugeiconsIcon icon={icons.back} size={ICON_SIZE.inline} />
      </button>
      <h1>{current}</h1>
      <span class="muted">{t('{n} 首', { n: rows.length })}</span>
      {#if !currentRoot && missingCount > 0}
        <button
          class="mini warn"
          title={t('这些歌缺歌词或封面，点开可以补齐')}
          onclick={() => openIntegrity(current!)}
        >
          {t('{n} 首缺内容', { n: missingCount })}
        </button>
      {/if}
      <div class="cloud-top-actions">
        {#if !currentRoot}
          <button
            class="mini btn-ico"
            onclick={() => openIntegrity(current!)}
            title={t('检查这个歌单里缺音频 / 歌词 / 封面的歌')}
          >
            <HugeiconsIcon icon={icons.shield} size={ICON_SIZE.inline} />
            {t('检查完整性')}
          </button>
        {/if}
        <button class="primary btn-ico" onclick={playAll} disabled={!rows.length}>
          <HugeiconsIcon icon={icons.play} size={ICON_SIZE.inline} />
          {t('播放全部')}
        </button>
        {#if canManage(current, currentRoot)}
          <button class="mini btn-ico" onclick={() => openRename(current!)}>
            <HugeiconsIcon icon={icons.edit} size={ICON_SIZE.inline} />
            {t('重命名')}
          </button>
        {/if}
      </div>
    </div>

    {#if err}<ErrorNotice text={err} />{/if}

    {#if detailSkel.visible}
      <CloudSkeleton mode="detail" rows={6} />
    {:else if detailLoading}
      <!-- 等骨架屏延迟 -->
    {:else if rows.length === 0}
      <div class="empty"><p>{t('这个歌单还是空的')}</p></div>
    {:else}
      <div class="s3list">
        {#each rows as r, i (r.key)}
          <div
            class="s3row"
            class:on={player.current?.id === r.key}
            role="group"
            oncontextmenu={(e) => rowMenu(e, r, i)}
          >
            <span class="idx">{i + 1}</span>
            {#if r.track.cover}
              <img class="s3cover" src={r.track.cover} alt="" loading="lazy" />
            {:else}
              <span class="s3cover ph">
                <HugeiconsIcon icon={icons.music} size={ICON_SIZE.inline} />
              </span>
            {/if}
            <span class="s3meta">
              <span class="s3title">
                {r.track.title || t('未命名')}
                {#if r.track.available_qualities?.includes('lossless')}
                  <span class="badge sq">{t('无损')}</span>
                {/if}
                {#if r.missing?.length}
                  <span
                    class="badge warn"
                    title={t('缺{parts}', { parts: partsText(r.missing) })}
                  >
                    {t('缺{parts}', { parts: partsText(r.missing) })}
                  </span>
                {/if}
              </span>
              <span class="s3sub">{r.track.artist || t('未知歌手')}</span>
            </span>
            {#if appCfg.s3Enabled}
              <button
                class="icon-btn fav"
                class:on={isFavorite(r.track)}
                title={isFavorite(r.track) ? t('取消喜欢') : t('喜欢（收藏到网盘）')}
                aria-label={isFavorite(r.track) ? t('取消喜欢') : t('喜欢')}
                aria-pressed={isFavorite(r.track)}
                onclick={() => toggleFav(r)}
              >
                <HugeiconsIcon icon={icons.favorite} size={ICON_SIZE.inline} />
              </button>
            {/if}
            <button
              class="playbtn"
              aria-label={isPlaying(r.key) ? t('暂停') : t('播放')}
              onclick={() => playRow(i)}
            >
              <MorphIcon
                icon={isPlaying(r.key) ? icons.pause : icons.play}
                size={ICON_SIZE.inline}
              />
            </button>
            {#if canManage(current, currentRoot)}
              <button
                class="icon-btn danger"
                title={t('删除这首歌')}
                aria-label={t('删除 {name}', { name: r.track.title })}
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
    <div class="modal narrow" role="dialog" aria-modal="true" aria-label={t('歌单操作')}>
      <div class="modal-head">
        <h2>
          {dialog.kind === 'create'
            ? t('新建歌单')
            : dialog.kind === 'rename'
              ? t('重命名歌单')
              : t('删除歌单')}
        </h2>
        <button class="icon" onclick={closeDialog} aria-label={t('关闭')} disabled={dialogBusy}>
          <HugeiconsIcon icon={icons.close} size={ICON_SIZE.inline} />
        </button>
      </div>

      {#if dialog.kind === 'delete'}
        <p class="modal-note">
          {t('删除「{name}」会同时删除云端 {n} 个文件，无法恢复。', {
            name: dialog.name,
            n: dialog.count
          })}
        </p>
      {:else}
        <input
          class="dialog-input"
          bind:value={dialogValue}
          placeholder={t('歌单名称')}
          spellcheck="false"
          maxlength="60"
          onkeydown={(e) => {
            if (e.key === 'Enter') submitDialog();
          }}
        />
      {/if}

      {#if dialogErr}<ErrorNotice text={dialogErr} compact />{/if}

      <div class="modal-actions">
        <button class="mini" onclick={closeDialog} disabled={dialogBusy}>{t('取消')}</button>
        <button
          class="mini"
          class:primary={dialog.kind !== 'delete'}
          class:danger={dialog.kind === 'delete'}
          onclick={submitDialog}
          disabled={dialogBusy || (dialog.kind !== 'delete' && !dialogValue.trim())}
        >
          {dialogBusy ? t('处理中…') : dialog.kind === 'delete' ? t('删除') : t('确定')}
        </button>
      </div>
    </div>
  </div>
{/if}
