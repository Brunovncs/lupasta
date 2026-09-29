// Drives the real app (Rust core + WebView2) through the Chrome DevTools Protocol.
// WebView2 exposes CDP when WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS carries a debugging port.
import { chromium, type Browser, type Page } from "playwright-core";
import { PNG } from "pngjs";
import { spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

export const ROOT = resolve(import.meta.dir, "../..");
export const FIXTURE = join(ROOT, "fixtures/visual/Users");
export const VIEW = { width: 906, height: 672, scale: 2 }; // → 1812×1344 screenshots

export interface App {
  page: Page;
  browser: Browser;
  proc: ChildProcess;
  dataDir: string;
  close(): Promise<void>;
}

export function exePath(): string {
  const rel = join(ROOT, "src-tauri/target/release/lupasta.exe");
  const dbg = join(ROOT, "src-tauri/target/debug/lupasta.exe");
  if (process.env.LUPASTA_EXE) return process.env.LUPASTA_EXE;
  if (process.env.LUPASTA_DEBUG && existsSync(dbg)) return dbg;
  if (existsSync(rel)) return rel;
  return dbg;
}

export async function launch(opts: { root?: string; select?: string; port?: number } = {}): Promise<App> {
  const port = opts.port ?? 9223 + Math.floor(Math.random() * 500);
  const dataDir = mkdtempSync(join(tmpdir(), "lupasta-e2e-"));
  const args = ["--root", opts.root ?? FIXTURE, "--data-dir", dataDir, "--capture"];
  if (opts.select) args.push("--select", opts.select);
  const proc = spawn(exePath(), args, {
    env: { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
    stdio: "ignore",
  });
  let browser: Browser | null = null;
  for (let i = 0; i < 100 && !browser; i++) {
    try {
      browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
    } catch {
      await Bun.sleep(150);
    }
  }
  if (!browser) throw new Error("could not attach to WebView2");
  let page: Page | undefined;
  for (let i = 0; i < 100 && !page; i++) {
    page = browser.contexts()[0]?.pages().find((p) => !p.url().startsWith("about:"));
    if (!page) await Bun.sleep(100);
  }
  if (!page) throw new Error("no page");
  // --capture sizes the window to 1812×1344 physical px and zooms to an effective DPR of 2.
  await page.waitForFunction(
    ({ w, h }) => innerWidth === w && innerHeight === h && Math.abs(devicePixelRatio - 2) < 0.01,
    { w: VIEW.width, h: VIEW.height },
    { timeout: 15000 },
  );
  await page.waitForFunction(() => (window as any).__lupasta?.ctl?.layout, null, { timeout: 15000 });
  await page.evaluate(() => document.fonts.ready);
  await settle(page);
  return {
    page,
    browser,
    proc,
    dataDir,
    async close() {
      await browser!.close().catch(() => {});
      proc.kill();
      await Bun.sleep(300);
      rmSync(dataDir, { recursive: true, force: true });
    },
  };
}

/** Waits until pending listings have arrived and the scene stopped animating. */
export async function settle(page: Page, extra = 80) {
  await page.waitForFunction(
    () => {
      const w = window as any;
      const ctl = w.__lupasta?.ctl;
      return ctl && !w.__lupasta.renderer.animating && ctl.inflight.size === 0 && ctl.model.missingForPreview(ctl.selectedId).length === 0;
    },
    null,
    { timeout: 10000 },
  );
  await Bun.sleep(extra);
}

/**
 * WebView2 captures in host DIPs and ignores page zoom, so Playwright's screenshot comes out at
 * monitor scale. Capturing the zoomed viewport's DIP rect at scale 1 yields native 1812×1344.
 */
export async function screenshot(page: Page, path: string): Promise<Buffer> {
  const cdp = await page.context().newCDPSession(page);
  const lm: any = await cdp.send("Page.getLayoutMetrics");
  const zoom: number = lm.cssVisualViewport.zoom ?? 1; // DIP per CSS px
  const shot = await cdp.send("Page.captureScreenshot", {
    format: "png",
    clip: { x: 0, y: 0, width: VIEW.width * zoom + 1, height: VIEW.height * zoom + 1, scale: 1 },
  });
  await cdp.detach();
  // Float rounding can leave the capture a pixel short or long: normalize to exactly W×H.
  const src = PNG.sync.read(Buffer.from(shot.data, "base64"));
  const W = VIEW.width * VIEW.scale;
  const H = VIEW.height * VIEW.scale;
  const out = new PNG({ width: W, height: H });
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const o = (y * W + x) * 4;
      if (x < src.width && y < src.height) src.data.copy(out.data, o, (y * src.width + x) * 4, (y * src.width + x) * 4 + 4);
      else out.data.writeUInt32BE(0x000000ff, o);
    }
  }
  const buf = PNG.sync.write(out);
  await Bun.write(path, buf);
  return buf;
}

export async function press(page: Page, ...keys: string[]) {
  for (const k of keys) {
    await page.keyboard.press(k);
    await settle(page, 20);
  }
  await settle(page);
}

export async function selected(page: Page): Promise<string> {
  return page.evaluate(() => (window as any).__lupasta.ctl.selectedId);
}

export async function select(page: Page, path: string) {
  await page.evaluate((p) => (window as any).__lupasta.ctl.reveal(p), path);
  await settle(page);
}
