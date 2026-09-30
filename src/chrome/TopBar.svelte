<script lang="ts">
  // One row at the top of the window that slides in when the pointer reaches the top edge (or
  // while settings are open). Same type and colours as the tree: no chrome of its own.
  import type { RootInfo, UpdateInfo } from "../state/backend";

  let {
    root,
    visible,
    settingsOpen,
    update,
    onSettings,
    onPickRoot,
    onUp,
    onSearch,
    onUpdate,
  }: {
    root: RootInfo | null;
    visible: boolean;
    settingsOpen: boolean;
    update: UpdateInfo | null;
    onSettings: () => void;
    onPickRoot: () => void;
    onUp: () => void;
    onSearch: () => void;
    onUpdate: () => void;
  } = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="bar" class:visible onmousedown={(e) => e.preventDefault()}>
  <span class="name">lupasta</span>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <span class="path item" title="open another folder (ctrl+o)" onclick={onPickRoot}>{root?.abs ?? ""}</span>
  <span class="spacer"></span>
  {#if update?.newer}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <span class="item update" onclick={onUpdate}>{update.latest} available</span>
  {/if}
  {#if root?.parent}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <span class="item" title="go up a folder" onclick={onUp}>up</span>
  {/if}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <span class="item" title="search (/)" onclick={onSearch}>search</span>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <span class="item" class:on={settingsOpen} title="settings (ctrl+,)" onclick={onSettings}>settings</span>
</div>

<style>
  .bar {
    position: fixed;
    left: 0;
    right: 0;
    top: 0;
    height: 28px;
    padding: 6px 20px;
    box-sizing: border-box;
    display: flex;
    gap: 3ch;
    align-items: center;
    font-size: var(--font-size);
    line-height: 16px;
    white-space: pre;
    color: #82808a;
    background: #000;
    box-shadow: 0 1px 0 #1c1b20;
    z-index: 20;
    transform: translateY(-100%);
    opacity: 0;
    transition:
      transform 160ms ease-out,
      opacity 160ms ease-out;
  }
  .bar.visible {
    transform: none;
    opacity: 1;
  }
  .name {
    color: #f06c04;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl; /* keep the end of a long path visible */
    text-align: left;
  }
  .spacer {
    flex: 1;
  }
  .item:hover,
  .item.on {
    color: #fcfcfc;
  }
  .update {
    color: #8ab0ff;
  }
</style>
