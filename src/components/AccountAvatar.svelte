<script lang="ts">
  import GeneratedAvatar from './GeneratedAvatar.svelte';

  interface Props {
    seed: string;
    /** 平台返回的真实头像；加载失败时退回自动生成的几何头像 */
    src?: string | null;
    size?: number;
  }

  let { seed, src = '', size = 28 }: Props = $props();

  let failed = $state(false);

  // 换账号或换头像地址时重新给图片一次机会
  $effect(() => {
    void src;
    failed = false;
  });

  const useImage = $derived(!!src && !failed);
</script>

{#if useImage}
  <img
    class="account-avatar-img"
    src={src}
    width={size}
    height={size}
    alt=""
    draggable="false"
    onerror={() => (failed = true)}
  />
{:else}
  <GeneratedAvatar {seed} {size} />
{/if}

<style>
  .account-avatar-img {
    display: block;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
</style>
