<script lang="ts">
  import type { Node } from "../tree/model";
  import { formatSize, relativeAge } from "../settings/prefs";

  let { node, now }: { node: Node | undefined; now: number } = $props();

  const text = $derived.by(() => {
    if (!node) return "";
    const when = node.mtime ? `modified ${relativeAge(node.mtime, now)}` : "";
    const what = node.isDirectory ? `${node.children?.length ?? "…"} items` : formatSize(node.size);
    return [what, when].filter(Boolean).join("   ");
  });
</script>

<div class="status">{text}</div>

<style>
  .status {
    position: fixed;
    left: 20px;
    bottom: 16px;
    height: 16px;
    line-height: 16px;
    font-size: var(--font-size);
    white-space: pre;
    color: #82808a;
    pointer-events: none;
  }
</style>
