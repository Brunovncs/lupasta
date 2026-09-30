<script lang="ts">
  import { onMount } from "svelte";
  import { backend, type IndexStatus, type SearchHit } from "../state/backend";
  import { clipDir, normalizeQuery, splitHit } from "./query";

  let { onPick, onClose }: { onPick: (path: string) => void; onClose: () => void } = $props();

  let input: HTMLInputElement;
  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let active = $state(0);
  let status = $state<IndexStatus | null>(null);
  const MAX_HITS = 12;
  let seq = 0;

  onMount(() => {
    input.focus();
    backend.indexStatus().then((s) => (status = s));
    const off = backend.onIndex((s) => (status = s));
    return () => void off.then((fns) => fns.forEach((f) => f()));
  });

  async function run(q: string) {
    const mine = ++seq;
    const norm = normalizeQuery(q);
    if (!norm) {
      hits = [];
      return;
    }
    const res = await backend.searchFiles(norm, MAX_HITS);
    if (mine !== seq) return; // a newer keystroke already went out
    hits = res.hits;
    active = 0;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
    else if (e.key === "ArrowDown") active = Math.min(hits.length - 1, active + 1);
    else if (e.key === "ArrowUp") active = Math.max(0, active - 1);
    else if (e.key === "Enter" && hits[active]) onPick(hits[active].path);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<div class="search">
  <div class="row">
    <span class="prompt">/</span><input
      bind:this={input}
      bind:value={query}
      oninput={() => run(query)}
      onkeydown={onKey}
      onblur={onClose}
      spellcheck="false"
      autocomplete="off"
    />
  </div>
  {#if status && status.state !== "ready"}
    <div class="row dim">{status.state} {status.indexed || status.corpus}</div>
  {/if}
  {#each hits as h, i (h.path)}
    {@const s = splitHit(h.path)}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="row hit" class:active={i === active} onmousedown={(e) => (e.preventDefault(), onPick(h.path))}>
      <span class="name"><span class="first">{s.name.slice(0, 1)}</span>{s.name.slice(1)}</span> <span class="dim">{clipDir(s.dir, 48)}</span>
    </div>
  {/each}
</div>

<style>
  .search {
    position: fixed;
    left: 20px;
    top: 16px;
    font-size: var(--font-size);
    line-height: 16px;
    color: #fcfcfc;
    background: #000;
    z-index: 10;
  }
  .row {
    height: 16px;
    white-space: pre;
    position: relative;
  }
  .prompt {
    color: #8ab0ff;
  }
  input {
    font: inherit;
    line-height: 16px;
    height: 16px;
    padding: 0;
    margin: 0;
    border: 0;
    outline: 0;
    color: inherit;
    background: transparent;
    caret-color: #e60000;
    width: 60ch;
  }
  .dim {
    color: #82808a;
  }
  .hit.active .first {
    background: #e60000;
  }
</style>
