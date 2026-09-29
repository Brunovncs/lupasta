import { describe, expect, test } from "bun:test";
import { Controller } from "../../src/state/controller";
import { metrics } from "../../src/scene/metrics";
import { NOW, REF_TREE, fakeBackend } from "./helpers";
import type { Layout } from "../../src/scene/layout";

const FB = "drcode/file-browser";

async function boot(initial: string | null = `${FB}/temp_0.md`) {
  const { api, calls } = fakeBackend(REF_TREE, initial);
  const ctl = new Controller(api, metrics, () => NOW);
  const layouts: Layout[] = [];
  ctl.attach({ setLayout: (l) => void layouts.push(l) });
  await ctl.init();
  await ctl.ensurePreview();
  await ctl.ensurePreview();
  return { ctl, calls, layouts };
}

const key = (k: string) => ({ key: k }) as KeyboardEvent;

describe("controller navigation", () => {
  test("starts at the initial path and batch-loads preview folders", async () => {
    const { ctl, calls } = await boot();
    expect(ctl.selectedId).toBe(`${FB}/temp_0.md`);
    expect(calls.some((c) => c.startsWith("batch:") && c.includes(`${FB}/Sources`) && c.includes(`${FB}/scripts`))).toBe(true);
    expect(ctl.layout!.nodes.has(`${FB}/Sources/App.swift`)).toBe(true);
  });

  test("without an initial path, selects the middle top-level entry", async () => {
    const { ctl } = await boot(null);
    expect(ctl.selectedId).toBe("drcode");
  });

  test("ArrowUp/Down move within the column and clamp at the ends", async () => {
    const { ctl } = await boot();
    ctl.key(key("ArrowUp"));
    expect(ctl.selectedId).toBe(`${FB}/Sources`);
    for (let i = 0; i < 20; i++) ctl.key(key("ArrowDown"));
    expect(ctl.selectedId).toBe(`${FB}/Tests`);
    ctl.key(key("Home"));
    expect(ctl.selectedId).toBe(`${FB}/.git`);
  });

  test("ArrowRight expands (enters) a directory on its centered child; ArrowLeft collapses", async () => {
    const { ctl } = await boot();
    ctl.key(key("ArrowUp")); // Sources
    await ctl.enter();
    expect(ctl.selectedId).toBe(`${FB}/Sources/FileBrowserView.swift`);
    expect(ctl.layout!.pathEdges.at(-1)).toEqual({ from: `${FB}/Sources`, to: `${FB}/Sources/FileBrowserView.swift` });
    ctl.key(key("ArrowLeft"));
    expect(ctl.selectedId).toBe(`${FB}/Sources`);
    expect(ctl.layout!.pathEdges.at(-1)!.to).toBe(`${FB}/Sources`);
  });

  test("re-entering remembers the last child", async () => {
    const { ctl } = await boot();
    ctl.key(key("ArrowUp"));
    await ctl.enter();
    ctl.key(key("ArrowUp"));
    expect(ctl.selectedId).toBe(`${FB}/Sources/FileBrowserModel.swift`);
    ctl.key(key("ArrowLeft"));
    await ctl.enter();
    expect(ctl.selectedId).toBe(`${FB}/Sources/FileBrowserModel.swift`);
  });

  test("Enter opens files through the backend; entering a file is a no-op", async () => {
    const { ctl, calls } = await boot();
    await ctl.activate();
    expect(calls).toContain(`open:${FB}/temp_0.md`);
    await ctl.enter();
    expect(ctl.selectedId).toBe(`${FB}/temp_0.md`);
  });

  test("lazy expansion: an unloaded directory is listed on demand", async () => {
    const { ctl, calls } = await boot();
    ctl.select(`${FB}/test-folders`);
    await ctl.ensurePreview();
    await ctl.enter();
    expect(ctl.selectedId.startsWith(`${FB}/test-folders/`)).toBe(true);
    expect(calls.some((c) => c.includes(`${FB}/test-folders/05 Mordor`))).toBe(true);
  });

  test("reveal loads every ancestor in one call and selects the target", async () => {
    const { ctl, calls } = await boot(null);
    expect(await ctl.reveal(`${FB}/test-folders/05 Mordor/roster.csv`)).toBe(true);
    expect(ctl.selectedId).toBe(`${FB}/test-folders/05 Mordor/roster.csv`);
    expect(ctl.layout!.nodes.get(`${FB}/test-folders/05 Mordor/roster.csv`)!.y).toBe(0);
    expect(calls.filter((c) => c.startsWith("list:")).length).toBe(1); // only the initial root listing
  });

  test("unreadable folders are asked for once, then shown empty", async () => {
    const { api, calls } = fakeBackend(REF_TREE, `${FB}/temp_0.md`, new Set([`${FB}/Tests`]));
    const ctl = new Controller(api, metrics, () => NOW);
    ctl.attach({ setLayout: () => {} });
    await ctl.init();
    await ctl.ensurePreview();
    expect(ctl.model.missingForPreview(ctl.selectedId)).toEqual([]);
    ctl.key(key("ArrowUp"));
    ctl.key(key("ArrowDown"));
    await ctl.ensurePreview();
    expect(calls.filter((c) => c.includes(`${FB}/Tests`)).length).toBe(1);
    ctl.select(`${FB}/Tests`);
    await ctl.enter(); // listing denied → stays put
    expect(ctl.selectedId).toBe(`${FB}/Tests`);
  });

  test("wheel steps selection every 40 units", async () => {
    const { ctl } = await boot();
    ctl.wheel(30);
    expect(ctl.selectedId).toBe(`${FB}/temp_0.md`);
    ctl.wheel(30);
    expect(ctl.selectedId).toBe(`${FB}/test-folders`);
  });

  test("refresh after a deletion falls back to the nearest surviving ancestor", async () => {
    const tree = structuredClone(REF_TREE) as any;
    const { api } = fakeBackend(tree, `${FB}/scripts/run.sh`);
    const ctl = new Controller(api, metrics, () => NOW);
    ctl.attach({ setLayout: () => {} });
    await ctl.init();
    delete tree.drcode["file-browser"].scripts;
    await ctl.refresh([FB]);
    expect(ctl.selectedId).toBe(FB);
  });
});
