<script lang="ts">
  import { onMount } from "svelte";
  import { backend } from "./state/backend";
  import { Controller } from "./state/controller";
  import { SceneRenderer } from "./scene/renderer";
  import { metrics } from "./scene/metrics";
  import Search from "./search/Search.svelte";

  let host: HTMLDivElement;
  let searching = $state(false);
  let error = $state("");
  const ctl = new Controller(backend, metrics);

  onMount(() => {
    if (!("__TAURI_INTERNALS__" in window)) {
      error = "lupasta needs the Rust core: run `bun tauri dev`.";
      return;
    }
    const renderer = new SceneRenderer(host, metrics, (id, e) => ctl.pick(id, e));
    ctl.attach(renderer);
    (window as any).__lupasta = { ctl, renderer, backend }; // e2e hook: reads layout/selection
    ctl.init().catch((e) => (error = String(e)));
    const unlisten = backend.onFilesystemChanged((dirs) => void ctl.refresh(dirs));

    const onKey = (e: KeyboardEvent) => {
      if (searching) return;
      if (e.key === "/" || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k")) {
        searching = true;
        e.preventDefault();
        return;
      }
      if (e.key === "Escape") return;
      if (ctl.key(e)) e.preventDefault();
    };
    const onWheel = (e: WheelEvent) => {
      if (searching) return;
      e.preventDefault();
      ctl.wheel(e.deltaY);
    };
    const onResize = () => renderer.resize();
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", onWheel);
      window.removeEventListener("resize", onResize);
      void unlisten.then((f) => f());
      renderer.destroy();
    };
  });

  async function pick(path: string) {
    searching = false;
    await ctl.reveal(path);
  }
</script>

<div bind:this={host} class:receded={searching}></div>
{#if searching}
  <Search onPick={pick} onClose={() => (searching = false)} />
{/if}
{#if error}
  <div class="error">{error}</div>
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
  }
</style>
