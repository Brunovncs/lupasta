import { afterEach, describe, expect, test } from "bun:test";
import { DEFAULTS, applyToMetrics, diff, formatSize, relativeAge, stepZoom } from "../../src/settings/prefs";
import { buildRows, cycle, nextFocus, type Context, type Row } from "../../src/settings/rows";
import { fitFont, type FontMetrics } from "../../src/styles/fonts";
import { AGE_MAX_MS, DEFAULT_AGE_MAX_MS, colorFor, colorMode } from "../../src/styles/palette";
import { metrics } from "../../src/scene/metrics";
import { Controller } from "../../src/state/controller";
import { FLAG_HIDDEN, type Listing } from "../../src/tree/model";
import { NOW, REF_TREE, fakeBackend } from "./helpers";

const ctx = (over: Partial<Context> = {}): Context => ({ settings: { ...DEFAULTS }, root: null, info: null, status: null, update: null, ...over });
const row = (rows: Row[], label: string) => rows.find((r) => "label" in r && r.label === label)!;

afterEach(() => applyToMetrics(DEFAULTS, { ...metrics }, false));

describe("prefs", () => {
  test("diff separates what needs a new listing from what needs a relayout", () => {
    const d = diff(DEFAULTS, { ...DEFAULTS, show_hidden: false, max_chars: 40 });
    expect(d).toEqual({ listing: true, layout: true, font: false, zoom: false, motion: false });
    expect(diff(DEFAULTS, { ...DEFAULTS, wheel_step: 20 })).toEqual({ listing: false, layout: false, font: false, zoom: false, motion: false });
  });

  test("applyToMetrics drives truncation, motion and the colour ramp", () => {
    const m = { ...metrics };
    applyToMetrics({ ...DEFAULTS, max_chars: 40, animation_ms: 400, color_mode: "kind", age_max_days: 30 }, m, false);
    expect([m.maxChars, m.duration, colorMode]).toEqual([40, 400, "kind"]);
    expect(AGE_MAX_MS).toBe(30 * 86400_000);
    expect(colorFor("code", 0, "path")).toBe("#8ab0ff");
    applyToMetrics(DEFAULTS, m, true);
    expect(m.duration).toBe(0); // prefers-reduced-motion wins
    expect(AGE_MAX_MS).toBe(DEFAULT_AGE_MAX_MS);
  });

  test("zoom steps snap to presets and clamp", () => {
    expect(stepZoom(1, 1)).toBe(1.1);
    expect(stepZoom(1, -1)).toBe(0.9);
    expect(stepZoom(1.2, 1)).toBe(1.25);
    expect(stepZoom(1.2, -1)).toBe(1.1);
    expect(stepZoom(2, 1)).toBe(2);
    expect(stepZoom(0.8, -1)).toBe(0.8);
  });

  test("status line formatting", () => {
    expect(relativeAge(NOW - 30_000, NOW)).toBe("just now");
    expect(relativeAge(NOW - 3 * 86400_000, NOW)).toBe("3 days ago");
    expect(relativeAge(NOW - 3600_000, NOW)).toBe("1 hour ago");
    expect([formatSize(12), formatSize(2048), formatSize(15 * 1024 * 1024)]).toEqual(["12 B", "2.0 KB", "15 MB"]);
  });
});

describe("settings rows", () => {
  test("choices reflect the current value and cycle with wrap-around", () => {
    const rows = buildRows(ctx());
    const hidden = row(rows, "hidden files");
    expect(hidden.kind === "choice" && hidden.options[hidden.index].label).toBe("show");
    const s = cycle(DEFAULTS, hidden, 1);
    expect(s.show_hidden).toBe(false);
    expect(cycle(s, row(buildRows(ctx({ settings: s })), "hidden files"), 1).show_hidden).toBe(true);
    const sort = row(rows, "sort by");
    expect(cycle(DEFAULTS, sort, -1).sort).toBe("size");
  });

  test("off-preset values select the nearest preset", () => {
    const rows = buildRows(ctx({ settings: { ...DEFAULTS, zoom: 1.3, age_max_days: 400 } }));
    const z = row(rows, "zoom");
    const a = row(rows, "oldest colour at");
    expect(z.kind === "choice" && z.options[z.index].label).toBe("125%");
    expect(a.kind === "choice" && a.options[a.index].label).toBe("1 year");
  });

  test("exclusions edit as a space-separated list", () => {
    const r = row(buildRows(ctx()), "skip folders");
    expect(r.kind).toBe("edit");
    if (r.kind !== "edit") return;
    expect(r.value.startsWith(".git node_modules")).toBe(true);
    expect(r.commit(DEFAULTS, "  .git   dist ").excludes).toEqual([".git", "dist"]);
  });

  test("folder rows: go up only with a parent; recent roots skip the current one", () => {
    const settings = { ...DEFAULTS, recent_roots: ["C:\\Users", "D:\\work"] };
    const rows = buildRows(ctx({ settings, root: { name: "Users", abs: "C:\\Users", parent: "C:\\", initial: null } }));
    expect(rows.some((r) => r.kind === "action" && r.action.type === "up-root")).toBe(true);
    const recent = rows.filter((r) => r.kind === "action" && r.action.type === "open-root");
    expect(recent.map((r) => (r.kind === "action" ? r.detail : ""))).toEqual(["D:\\work"]);
    const top = buildRows(ctx({ root: { name: "C:\\", abs: "C:\\", parent: null, initial: null } }));
    expect(top.some((r) => r.kind === "action" && r.action.type === "up-root")).toBe(false);
  });

  test("update row turns into a download link only for a newer release", () => {
    const newer = buildRows(ctx({ update: { current: "0.1.0", latest: "v0.2.0", newer: true, url: "https://github.com/Brunovncs/lupasta/releases/tag/v0.2.0" } }));
    expect(newer.some((r) => r.kind === "action" && r.action.type === "open-release")).toBe(true);
    const same = row(buildRows(ctx({ update: { current: "0.1.0", latest: "v0.1.0", newer: false, url: null } })), "check for updates");
    expect(same.kind === "action" && same.detail).toBe("up to date (v0.1.0)");
  });

  test("focus skips headings and info rows", () => {
    const rows = buildRows(ctx());
    const first = nextFocus(rows, -1, 1);
    expect(rows[first].kind).toBe("choice");
    expect(nextFocus(rows, first, -1)).toBe(first);
    const last = nextFocus(rows, rows.length, -1);
    expect(rows[last].kind).toBe("action"); // the key list at the end is informational
  });
});

describe("font fitting", () => {
  const iosevka: FontMetrics = { advance: 0.5, ellipsis: 1, ascent: 0.965, descent: 0.285, cap: 0.735 };

  test("the reference font keeps the measured geometry", () => {
    expect(fitFont(iosevka, iosevka, metrics)).toEqual({ fontSize: 20, textY: -1.5, ellScale: 0.5 });
  });

  test("a wider font is shrunk to one 10 px cell and re-centred", () => {
    const wide: FontMetrics = { advance: 0.6, ellipsis: 0.6, ascent: 1.02, descent: 0.3, cap: 0.73 };
    const fit = fitFont(wide, iosevka, metrics);
    expect(fit.fontSize).toBeCloseTo(16.667, 2);
    expect(fit.ellScale).toBe(1); // its "…" already fits one cell
    expect(Math.abs(fit.textY)).toBeLessThan(4);
  });
});

describe("controller hooks", () => {
  test("reload drops entries the backend no longer lists and keeps a valid selection", async () => {
    const { api } = fakeBackend(REF_TREE, "drcode/file-browser/.gitignore");
    let hide = false;
    const strip = (l: Listing): Listing => (hide ? { ...l, entries: l.entries.filter((e) => !(e[1] & FLAG_HIDDEN) && !e[0].startsWith(".")) } : l);
    const filtered = {
      ...api,
      listDirectory: async (p: string) => strip(await api.listDirectory(p)),
      listDirectories: async (ps: string[]) => (await api.listDirectories(ps)).map(strip),
    };
    const ctl = new Controller(filtered, metrics, () => NOW);
    ctl.attach({ setLayout: () => {} });
    await ctl.init();
    expect(ctl.selectedId).toBe("drcode/file-browser/.gitignore");
    hide = true;
    await ctl.reload();
    expect(ctl.model.get("drcode/file-browser/.gitignore")).toBeUndefined();
    expect(ctl.model.get("drcode/file-browser/.git")).toBeUndefined();
    expect(ctl.selectedId).toBe("drcode/file-browser");
  });

  test("left arrow on a top-level entry asks the shell to go up; select() notifies", async () => {
    const { api } = fakeBackend(REF_TREE, "drcode");
    const ctl = new Controller(api, metrics, () => NOW);
    const seen: string[] = [];
    let up = 0;
    ctl.onSelect = (id) => void seen.push(id);
    ctl.onLeaveRoot = () => void up++;
    ctl.attach({ setLayout: () => {} });
    await ctl.init();
    ctl.leave();
    expect(up).toBe(1);
    ctl.key({ key: "ArrowUp" } as KeyboardEvent);
    expect(seen.at(-1)).toBe(".localized");
  });

  test("answers for a previous root are ignored after init()", async () => {
    const { api } = fakeBackend(REF_TREE, null);
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    const slow = { ...api, revealPath: async (p: string) => (await gate, api.revealPath(p)) };
    const ctl = new Controller(slow, metrics, () => NOW);
    ctl.attach({ setLayout: () => {} });
    await ctl.init();
    const stale = ctl.reveal("drcode/file-browser/Sources/App.swift");
    await ctl.init(); // root changed meanwhile
    release();
    expect(await stale).toBe(false);
    // The new root's own preview may list Sources, but the stale reveal must not have loaded it.
    expect(ctl.model.get("drcode/file-browser/Sources")?.loaded ?? false).toBe(false);
    expect(ctl.selectedId).not.toBe("drcode/file-browser/Sources/App.swift");
  });
});
