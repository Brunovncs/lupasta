// End-to-end + visual regression against the real app.
//   bun tests/e2e/run.ts            run, compare with baselines and reference frames
//   bun tests/e2e/run.ts --update   also (re)write baselines
import { appendFileSync, copyFileSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { buildFixture } from "../../scripts/make-fixture";
import { FIXTURE, ROOT, launch, press, screenshot, selected, settle } from "./harness";
import { diffRatio, inkAgreement, overlay, readPng } from "./compare";
import type { Page } from "playwright-core";

const UPDATE = process.argv.includes("--update");
const SHOTS = join(ROOT, "screenshots");
const CUR = join(SHOTS, "current");
const BASE = join(SHOTS, "baseline");
const CMP = join(SHOTS, "compare");
for (const d of [CUR, BASE, CMP]) mkdirSync(d, { recursive: true });

const FB = "drcode/file-browser";
const rows: string[] = [];
let failures = 0;

function check(cond: boolean, what: string) {
  console.log(`${cond ? "PASS" : "FAIL"}  ${what}`);
  if (!cond) failures++;
}

async function expectSelected(page: Page, path: string, what: string) {
  check((await selected(page)) === path, `${what} → ${path}`);
}

const hasNode = (page: Page, id: string) => page.evaluate((i) => (window as any).__lupasta.ctl.layout.nodes.has(i), id);

async function snap(page: Page, name: string, ref?: string) {
  await settle(page, 120);
  const file = join(CUR, `${name}.png`);
  await screenshot(page, file);
  const cur = readPng(file);
  let refCell = "—";
  // Reference frames come from a third-party recording and are not distributed with the repo.
  if (ref && existsSync(join(SHOTS, "reference", ref))) {
    const r = readPng(join(SHOTS, "reference", ref));
    overlay(r, cur, join(CMP, `${name}.png`));
    refCell = `${ref} · ink agreement ${(inkAgreement(r, cur) * 100).toFixed(1)}%`;
  }
  const basePath = join(BASE, `${name}.png`);
  let baseCell = "new";
  if (existsSync(basePath) && !UPDATE) {
    const ratio = diffRatio(readPng(basePath), cur, join(CMP, `${name}.diff.png`));
    baseCell = `${(ratio * 100).toFixed(3)}%`;
    check(ratio < 0.002, `visual regression ${name} (${baseCell} px differ)`);
  } else copyFileSync(file, basePath);
  rows.push(`| ${name} | ${refCell} | ${baseCell} |`);
}

// Generous: watcher checks include the 120 ms debounce plus IPC, and the machine may be busy.
async function waitFor(page: Page, fn: () => Promise<boolean>, ms = 10000) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (await fn()) return true;
    await Bun.sleep(50);
  }
  return false;
}

buildFixture(FIXTURE);
const app = await launch({ select: `${FB}/temp_0.md` });
const { page } = app;
const t0 = Date.now();
try {
  // 1. initial tree (reference: t=0 of the video)
  await expectSelected(page, `${FB}/temp_0.md`, "launch --select");
  check(await hasNode(page, `${FB}/Sources/OrthogonalRouter.swift`), "preview column lists Sources children");
  check(await hasNode(page, `${FB}/test-folders/05 Mordor`), "preview column lists test-folders children");
  await snap(page, "01-initial", "f_001.png");

  // 2. Sources expanded
  await press(page, "ArrowUp");
  await expectSelected(page, `${FB}/Sources`, "ArrowUp");
  await press(page, "ArrowRight");
  await expectSelected(page, `${FB}/Sources/FileBrowserView.swift`, "ArrowRight enters on the centered child");
  await snap(page, "02-sources-expanded");
  await press(page, "ArrowLeft");
  await expectSelected(page, `${FB}/Sources`, "ArrowLeft returns to parent");

  // 3. test-folders expanded (level-2 previews)
  await press(page, "ArrowDown", "ArrowDown");
  await expectSelected(page, `${FB}/test-folders`, "ArrowDown ×2");
  check(await hasNode(page, `${FB}/test-folders/05 Mordor/Mount Doom`), "level-2 preview lists grandchildren");
  await snap(page, "03-test-folders-expanded", "f_011.png");

  // 4. deep nested branch
  await press(page, "ArrowRight");
  await expectSelected(page, `${FB}/test-folders/07 Isengard`, "enter test-folders");
  await snap(page, "x-07-isengard", "f_006.png");
  await press(page, "ArrowUp", "ArrowUp");
  await snap(page, "x-05-mordor", "f_007.png");
  await press(page, "Enter");
  await expectSelected(page, `${FB}/test-folders/05 Mordor/One does not simply file a permit.txt`, "Enter expands directory");
  await snap(page, "04-deep-branch", "f_008.png");
  await press(page, "ArrowUp", "ArrowRight");
  await expectSelected(page, `${FB}/test-folders/05 Mordor/Mount Doom/One Ring Returns Desk`, "deeper");
  await snap(page, "x-one-ring", "f_010.png");

  // 5. selected node (mouse): back to scripts, click build.sh in the preview
  await press(page, "ArrowLeft", "ArrowLeft", "ArrowLeft");
  await expectSelected(page, `${FB}/test-folders`, "ArrowLeft ×3 collapses back");
  check(!(await hasNode(page, `${FB}/test-folders/05 Mordor/Mount Doom/One Ring Returns Desk/Claim form.pdf`)), "collapsed levels are gone");
  await press(page, "ArrowUp", "ArrowUp", "ArrowUp");
  await expectSelected(page, `${FB}/scripts`, "ArrowUp ×3");
  await snap(page, "x-scripts", "f_005.png");
  await page.locator(`[data-id="${FB}/scripts/build.sh"]`).click();
  await settle(page);
  await expectSelected(page, `${FB}/scripts/build.sh`, "mouse click selects");
  await snap(page, "05-selected-node", "f_016.png");

  // 8. collapsed state
  await press(page, "ArrowLeft", "ArrowUp");
  await expectSelected(page, `${FB}/README.md`, "collapse + move");
  await snap(page, "08-collapsed", "f_004.png");
  await press(page, "ArrowDown", "ArrowDown", "ArrowDown", "ArrowDown", "ArrowDown");
  await expectSelected(page, `${FB}/Tests`, "ArrowDown ×5");
  await snap(page, "x-tests", "f_012.png");

  // Search (index must be ready)
  const idxReady = await waitFor(page, () => page.evaluate(async () => (await (window as any).__lupasta.backend.indexStatus()).state === "ready"), 15000);
  check(idxReady, "background index ready");
  const search = (q: string) => page.evaluate(async (qq) => (await (window as any).__lupasta.backend.searchFiles(qq, 10)).hits, q);
  const router = await search("Router");
  check(router.some((h: any) => h.name === "OrthogonalRouter.swift"), `search "Router" finds OrthogonalRouter.swift (top: ${router[0]?.name})`);
  const rtr = await search("rtr");
  check(rtr.slice(0, 5).some((h: any) => h.name === "OrthogonalRouter.swift"), `search "rtr" finds OrthogonalRouter.swift in top 5`);
  const fb = await search("file browser");
  check(fb[0]?.path === FB, `search "file browser" → ${fb[0]?.path}`);

  // 6. search opened
  await page.keyboard.press("/");
  await page.keyboard.type("rtr", { delay: 30 });
  await waitFor(page, async () => (await page.locator(".search .hit").count()) > 0);
  await snap(page, "06-search-opened");
  // 7. search result selected
  const names = await page.locator(".search .hit .name").allTextContents();
  const idx = names.indexOf("OrthogonalRouter.swift");
  for (let i = 0; i < idx; i++) await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await settle(page);
  await expectSelected(page, `${FB}/Sources/OrthogonalRouter.swift`, "reveal search result");
  check((await page.locator(".search").count()) === 0, "search closes after picking");
  await snap(page, "07-search-result-selected");

  // Escape closes search
  await page.keyboard.press("Control+k");
  check((await page.locator(".search").count()) === 1, "Ctrl+K opens search");
  await page.keyboard.press("Escape");
  check((await page.locator(".search").count()) === 0, "Escape closes search");

  // Watcher: create → modify → delete, visible tree + search follow
  const probe = join(FIXTURE, FB, "Sources", "Zeta.swift");
  const probeId = `${FB}/Sources/Zeta.swift`;
  writeFileSync(probe, "// new\n");
  check(await waitFor(page, () => hasNode(page, probeId)), "created file appears in the tree");
  check(await waitFor(page, async () => (await search("Zeta")).some((h: any) => h.path === probeId)), "created file is searchable");
  const colorOf = () => page.evaluate((i) => (window as any).__lupasta.ctl.layout.nodes.get(i)?.color, `${FB}/Sources/App.swift`);
  const before = await colorOf();
  appendFileSync(join(FIXTURE, FB, "Sources", "App.swift"), "// touched\n");
  check(await waitFor(page, async () => (await colorOf()) !== before), "modified file recolors (recency)");
  rmSync(probe);
  check(await waitFor(page, async () => !(await hasNode(page, probeId))), "deleted file leaves the tree");
  check(await waitFor(page, async () => !(await search("Zeta")).some((h: any) => h.path === probeId)), "deleted file leaves the index");

  // Animation sanity: frames are produced when navigating
  const frames = await page.evaluate(async () => {
    const r = (window as any).__lupasta.renderer;
    const f0 = r.frames;
    (window as any).__lupasta.ctl.move(1);
    await new Promise((res) => setTimeout(res, 400));
    return r.frames - f0;
  });
  check(frames > 5, `navigation animates (${frames} frames)`);
} catch (e) {
  failures++;
  console.error(e);
} finally {
  await app.close();
}

const report = [
  "| state | reference | vs baseline |",
  "|---|---|---|",
  ...rows,
].join("\n");
writeFileSync(join(SHOTS, "report.md"), `# Visual report\n\nGenerated ${new Date().toISOString()} in ${((Date.now() - t0) / 1000).toFixed(1)} s.\nOverlays in screenshots/compare (red = reference only, cyan = ours only).\n\n${report}\n`);
console.log("\n" + report);
console.log(failures ? `\n${failures} failure(s)` : "\nall e2e checks passed");
process.exit(failures ? 1 : 0);
