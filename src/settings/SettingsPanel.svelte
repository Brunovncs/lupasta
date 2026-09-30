<script lang="ts">
  import { tick } from "svelte";
  import type { Settings } from "../state/backend";
  import { buildRows, cycle, focusable, nextFocus, type Action, type Context, type Row } from "./rows";

  let {
    ctx,
    onChange,
    onAction,
    onClose,
  }: { ctx: Context; onChange: (s: Settings) => void; onAction: (a: Action) => void; onClose: () => void } = $props();

  const rows = $derived(buildRows(ctx));
  // Rows whose options would not fit next to the label show only the current one: ‹ value ›.
  const WIDE = 34;
  const compact = (row: Row) => row.kind === "choice" && row.options.reduce((n, o) => n + o.label.length + 2, -2) > WIDE;
  let active = $state(-1);
  let editing = $state(false);
  let draft = $state("");
  let panel: HTMLDivElement;
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (active < 0 || active >= rows.length || !focusable(rows[active])) active = nextFocus(rows, -1, 1);
  });

  $effect(() => {
    const el = panel?.querySelector<HTMLElement>(`[data-row="${active}"]`);
    el?.scrollIntoView({ block: "nearest" });
  });

  function activate(row: Row) {
    if (row.kind === "choice") onChange(cycle(ctx.settings, row, 1));
    else if (row.kind === "action") onAction(row.action);
    else if (row.kind === "edit") startEdit(row.value);
  }

  async function startEdit(value: string) {
    draft = value;
    editing = true;
    await tick();
    input?.focus();
    input?.select();
  }

  function commit() {
    if (!editing) return; // Escape already cancelled (WebKit fires blur when the input goes away)
    const row = rows[active];
    if (row?.kind === "edit") onChange(row.commit(ctx.settings, draft));
    editing = false;
  }

  function onKey(e: KeyboardEvent) {
    if (editing) {
      if (e.key === "Enter") commit();
      else if (e.key === "Escape") editing = false;
      else return;
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    const row = rows[active];
    if (e.key === "Escape" || (e.key === "," && (e.ctrlKey || e.metaKey))) onClose();
    else if (e.key === "ArrowDown") active = nextFocus(rows, active, 1);
    else if (e.key === "ArrowUp") active = nextFocus(rows, active, -1);
    else if (e.key === "Home") active = nextFocus(rows, -1, 1);
    else if (e.key === "End") active = nextFocus(rows, rows.length, -1);
    else if (e.key === "ArrowRight" && row?.kind === "choice") onChange(cycle(ctx.settings, row, 1));
    else if (e.key === "ArrowLeft" && row?.kind === "choice") onChange(cycle(ctx.settings, row, -1));
    else if ((e.key === "Enter" || e.key === " ") && row) activate(row);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }

  function pickOption(i: number, row: Row, j: number) {
    active = i;
    if (row.kind === "choice") onChange(row.options[j].apply(ctx.settings));
  }
</script>

<svelte:window onkeydowncapture={onKey} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="panel" bind:this={panel} onwheel={(e) => e.stopPropagation()}>
  {#each rows as row, i (i)}
    {#if row.kind === "head"}
      <div class="row head" class:gap={i > 0}>{row.label}</div>
    {:else}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="row item"
        class:active={i === active}
        data-row={i}
        onmousedown={(e) => {
          e.preventDefault();
          if (!focusable(row)) return;
          active = i;
          activate(row);
        }}
      >
        <span class="label"><span class="first">{row.label.slice(0, 1)}</span>{row.label.slice(1)}</span>
        {#if row.kind === "choice" && compact(row)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <span class="opt" onmousedown={(e) => (e.preventDefault(), e.stopPropagation(), (active = i), onChange(cycle(ctx.settings, row, -1)))}>‹</span
          >{" "}<span class="opt on">{row.options[row.index].label}</span>{" "}<!-- svelte-ignore a11y_click_events_have_key_events --><span
            class="opt"
            onmousedown={(e) => (e.preventDefault(), e.stopPropagation(), (active = i), onChange(cycle(ctx.settings, row, 1)))}>›</span
          >
        {:else if row.kind === "choice"}
          {#each row.options as opt, j (j)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <span
              class="opt"
              class:on={j === row.index}
              onmousedown={(e) => (e.preventDefault(), e.stopPropagation(), pickOption(i, row, j))}>{opt.label}</span
            >{#if j < row.options.length - 1}{"  "}{/if}
          {/each}
        {:else if row.kind === "edit"}
          {#if editing && i === active}
            <input
              bind:this={input}
              bind:value={draft}
              onblur={commit}
              onmousedown={(e) => e.stopPropagation()}
              spellcheck="false"
              autocomplete="off"
            />
          {:else}
            <span class="detail">{row.value || "—"}</span>
          {/if}
        {:else}
          <span class="detail">{row.detail ?? ""}</span>
        {/if}
      </div>
    {/if}
  {/each}
</div>

<style>
  .panel {
    position: fixed;
    inset: 28px 0 0 0;
    padding: 16px 20px;
    box-sizing: border-box;
    background: rgb(0 0 0 / 0.9);
    overflow-y: auto;
    font-size: var(--font-size);
    line-height: 16px;
    color: #82808a;
    z-index: 10;
  }
  .row {
    height: 16px;
    white-space: pre;
    /* clip, not hidden: rows are shorter than the glyphs, and hidden would cut descenders */
    overflow-x: clip;
    text-overflow: ellipsis;
  }
  .head {
    color: #8ab0ff;
  }
  .head.gap {
    margin-top: 16px;
  }
  .item {
    padding-left: 2ch;
    cursor: default;
  }
  .label {
    display: inline-block;
    width: 22ch;
    color: #fcfcfc;
  }
  .item.active .first {
    background: #e60000;
  }
  .opt.on {
    color: #fcfcfc;
  }
  .opt:hover {
    color: #fcfcfc;
  }
  input {
    font: inherit;
    line-height: 16px;
    height: 16px;
    padding: 0;
    margin: 0;
    border: 0;
    outline: 0;
    color: #fcfcfc;
    background: transparent;
    caret-color: #e60000;
    width: 60ch;
  }
</style>
