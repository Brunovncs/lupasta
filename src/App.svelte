<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { backend, type AppInfo, type IndexStatus, type RootInfo, type Settings, type UpdateInfo } from "./state/backend";
  import { Controller } from "./state/controller";
  import { SceneRenderer } from "./scene/renderer";
  import { metrics } from "./scene/metrics";
  import { applyFont } from "./styles/fonts";
  import { DEFAULTS, applyToMetrics, diff, stepZoom } from "./settings/prefs";
  import type { Action, Context } from "./settings/rows";
  import type { Node } from "./tree/model";
  import Search from "./search/Search.svelte";
  import SettingsPanel from "./settings/SettingsPanel.svelte";
  import TopBar from "./chrome/TopBar.svelte";
  import StatusLine from "./chrome/StatusLine.svelte";

  let host: HTMLDivElement;
  let searching = $state(false);
  let settingsOpen = $state(false);
  let barHover = $state(false);
  let error = $state("");
  let notice = $state("");
  let settings = $state<Settings>({ ...DEFAULTS });
  let root = $state<RootInfo | null>(null);
  let info = $state<AppInfo | null>(null);
  let status = $state<IndexStatus | null>(null);
  let update = $state<Context["update"]>(null);
  let selected = $state<Node | undefined>(undefined);
  let tickNow = $state(Date.now());
  const ctl = new Controller(backend, metrics);
  let renderer: SceneRenderer | null = null;
  const reducedMotion = matchMedia("(prefers-reduced-motion: reduce)");

  const ctx = $derived<Context>({ settings, root, info, status, update });
  const available = $derived(update && typeof update === "object" && "newer" in update ? (update as UpdateInfo) : null);

  function flash(text: string) {
    notice = text;
    const mine = text;
    setTimeout(() => notice === mine && (notice = ""), 2200);
  }

  async function applyZoom(z: number) {
    if (info?.capture) return; // the e2e harness owns the zoom (effective DPR 2)
    try {
      await getCurrentWebview().setZoom(z);
    } catch (e) {
      console.warn("zoom", e);
    }
  }

  /** Applies settings to the running scene; `prev` = what is currently applied. */
  async function apply(next: Settings, prev: Settings | null) {
    const ch = prev ? diff(prev, next) : { listing: false, layout: true, font: true, zoom: true, motion: true };
    applyToMetrics(next, metrics, reducedMotion.matches);
    ctl.wheelStep = next.wheel_step;
    if (ch.font) await applyFont(next.font, metrics);
    renderer?.applyMetrics();
    if (ch.zoom) await applyZoom(next.zoom);
    if (ch.listing) await ctl.reload();
    else if (ch.layout || ch.font) ctl.relayout(ch.font);
  }

  async function change(next: Settings) {
    const prev = settings;
    settings = next;
    try {
      settings = await backend.setSettings(next);
    } catch (e) {
      flash(String(e));
    }
    await apply(settings, prev);
  }

  async function rootChanged(info: RootInfo) {
    root = info;
    searching = false;
    await ctl.init();
    status = await backend.indexStatus();
    settings = await backend.getSettings(); // recent roots / last location changed
  }

  async function setRoot(path: string, select: string | null = null) {
    try {
      await rootChanged(await backend.setRoot(path, select));
    } catch (e) {
      flash(String(e));
    }
  }

  async function pickRoot() {
    const dir = await openDialog({ directory: true, defaultPath: root?.abs, title: "Open folder in lupasta" });
    if (typeof dir === "string") await setRoot(dir);
  }

  async function upRoot() {
    if (root?.parent) await setRoot(root.parent, root.name);
  }

  async function checkUpdate() {
    update = "checking";
    try {
      update = await backend.checkUpdate();
    } catch (e) {
      update = { error: String(e) };
    }
  }

  async function runAction(a: Action) {
    switch (a.type) {
      case "pick-root":
        return pickRoot();
      case "up-root":
        return upRoot();
      case "open-root":
        return setRoot(a.path);
      case "reindex":
        await backend.startIndexing();
        status = await backend.indexStatus();
        return;
      case "clear-index":
        await backend.clearIndex();
        status = await backend.indexStatus();
        return;
      case "check-update":
        return checkUpdate();
      case "open-release":
        return backend.openRelease(a.url);
      case "open-data":
        return backend.openDataDir();
    }
  }

  async function copyPath() {
    const n = ctl.selected;
    if (!n) return;
    try {
      const abs = await backend.absPath(n.id);
      await writeText(abs);
      flash(`copied ${abs}`);
    } catch (e) {
      flash(String(e));
    }
  }

  /** Shell-level shortcuts; returns true when handled. */
  function shortcut(e: KeyboardEvent): boolean {
    const mod = e.ctrlKey || e.metaKey;
    const k = e.key.toLowerCase();
    if (mod && k === ",") return (settingsOpen = !settingsOpen), (searching = false), true;
    if (settingsOpen) return false;
    if (e.key === "/" || (mod && k === "k")) return (searching = true), true;
    if (!mod) return false;
    if (k === "c") return void copyPath(), true;
    if (k === "e") return ctl.selected ? (void backend.revealInOs(ctl.selected.id), true) : true;
    if (k === "h") {
      void change({ ...settings, show_hidden: !settings.show_hidden });
      flash(settings.show_hidden ? "hidden files shown" : "hidden files hidden");
      return true;
    }
    if (k === "o") return void pickRoot(), true;
    if (k === "=" || k === "+") return void change({ ...settings, zoom: stepZoom(settings.zoom, 1) }), true;
    if (k === "-") return void change({ ...settings, zoom: stepZoom(settings.zoom, -1) }), true;
    if (k === "0") return void change({ ...settings, zoom: 1 }), true;
    // Reloading the page would drop all state; the webview offers it by default.
    if (k === "r") return true;
    return false;
  }

  /** `--smoke`: prove the real app renders and searches on this platform, then exit. */
  async function smoke() {
    try {
      // Previews load asynchronously after init: wait until none is missing.
      for (let i = 0; i < 100 && ctl.model.missingForPreview(ctl.selectedId).length; i++) await new Promise((r) => setTimeout(r, 50));
      const nodes = ctl.layout?.nodes.size ?? 0;
      if (!nodes) throw new Error("no layout after init");
      const first = ctl.model.children("")?.[0];
      if (!first) throw new Error("root listing is empty");
      for (let i = 0; i < 600 && (await backend.indexStatus()).state !== "ready"; i++) await new Promise((r) => setTimeout(r, 100));
      const res = await backend.searchFiles(first.name, 5);
      if (!res.hits.some((h) => h.path === first.id)) throw new Error(`search for "${first.name}" did not find it (${res.strategy})`);
      await backend.smokeReport(true, `${nodes} nodes laid out, search found ${first.id}`);
    } catch (e) {
      await backend.smokeReport(false, String(e));
    }
  }

  onMount(() => {
    if (!("__TAURI_INTERNALS__" in window)) {
      error = "lupasta needs the Rust core: run `bun tauri dev`.";
      return;
    }
    renderer = new SceneRenderer(host, metrics, (id, e) => ctl.pick(id, e));
    ctl.attach(renderer);
    (window as any).__lupasta = { ctl, renderer, backend }; // e2e hook: reads layout/selection

    let rememberTimer = 0;
    ctl.onSelect = (id) => {
      clearTimeout(rememberTimer);
      rememberTimer = window.setTimeout(() => void backend.rememberSelection(id).catch(() => {}), 800);
    };
    ctl.onChange = () => {
      selected = ctl.selected;
      tickNow = Date.now();
    };
    ctl.onLeaveRoot = () => void upRoot();

    (async () => {
      [info, settings, root] = await Promise.all([backend.appInfo(), backend.getSettings(), backend.getRoot()]);
      await apply(settings, null);
      await ctl.init();
      status = await backend.indexStatus();
      if (info.smoke) await smoke();
    })().catch((e) => (error = String(e)));

    const unlisten = backend.onFilesystemChanged((dirs) => void ctl.refresh(dirs));
    const unIndex = backend.onIndex((s) => (status = s));

    const onKey = (e: KeyboardEvent) => {
      if (searching) return;
      if (shortcut(e)) {
        e.preventDefault();
        return;
      }
      if (settingsOpen || e.key === "Escape") return;
      if (ctl.key(e)) e.preventDefault();
    };
    const onWheel = (e: WheelEvent) => {
      if (searching || settingsOpen) return;
      e.preventDefault();
      ctl.wheel(e.deltaY);
    };
    let hideTimer = 0;
    const onMove = (e: MouseEvent) => {
      // The e2e screenshots click near the top edge; the bar stays reachable via ctrl+,.
      if (info?.capture) return;
      if (e.clientY <= 28) {
        clearTimeout(hideTimer);
        barHover = true;
      } else if (barHover && e.clientY > 44) {
        clearTimeout(hideTimer);
        hideTimer = window.setTimeout(() => (barHover = false), 350);
      }
    };
    const onLeave = () => {
      clearTimeout(hideTimer);
      hideTimer = window.setTimeout(() => (barHover = false), 350);
    };
    const onResize = () => renderer?.resize();
    const onMotion = () => void apply(settings, null);
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("resize", onResize);
    window.addEventListener("mousemove", onMove);
    document.documentElement.addEventListener("mouseleave", onLeave);
    reducedMotion.addEventListener("change", onMotion);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", onWheel);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("mousemove", onMove);
      document.documentElement.removeEventListener("mouseleave", onLeave);
      reducedMotion.removeEventListener("change", onMotion);
      void unlisten.then((f) => f());
      void unIndex.then((fns) => fns.forEach((f) => f()));
      renderer?.destroy();
    };
  });

  async function pick(path: string) {
    searching = false;
    await ctl.reveal(path);
  }
</script>

<div bind:this={host} class:receded={searching || settingsOpen}></div>
<TopBar
  {root}
  visible={barHover || settingsOpen}
  {settingsOpen}
  update={available}
  onSettings={() => ((settingsOpen = !settingsOpen), (searching = false))}
  onPickRoot={pickRoot}
  onUp={upRoot}
  onSearch={() => ((settingsOpen = false), (searching = true))}
  onUpdate={() => available?.url && backend.openRelease(available.url)}
/>
{#if searching}
  <Search onPick={pick} onClose={() => (searching = false)} />
{/if}
{#if settingsOpen}
  <SettingsPanel {ctx} onChange={change} onAction={runAction} onClose={() => (settingsOpen = false)} />
{/if}
{#if settings.status_line && !settingsOpen && !searching && !error && !notice}
  <StatusLine node={selected} now={tickNow} />
{/if}
{#if error || notice}
  <div class="error">{error || notice}</div>
{/if}

<style>
  /* While searching, the tree steps back so the result list reads on top of it. */
  :global(.scene.receded) {
    opacity: 0.25;
  }
  .error {
    position: fixed;
    left: 20px;
    bottom: 16px;
    color: #82808a;
    font-size: 14px;
    white-space: pre;
  }
</style>
