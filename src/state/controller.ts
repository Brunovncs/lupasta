// Navigation state: selection, lazy loading and keyboard/mouse intent. Owns the TreeModel and
// feeds layouts to the renderer; knows nothing about DOM details.
import { TreeModel, parentOf, type Listing } from "../tree/model";
import { computeLayout, entryChildIndex, type Layout } from "../scene/layout";
import type { Metrics } from "../scene/metrics";
import type { Backend } from "./backend";

export interface LayoutSink {
  setLayout(layout: Layout, instant?: boolean): void;
}

export class Controller {
  model = new TreeModel();
  selectedId = "";
  layout: Layout | null = null;
  private lastChild = new Map<string, string>();
  private inflight = new Set<string>();
  private sink: LayoutSink | null = null;
  private wheelAcc = 0;
  onChange: () => void = () => {};

  constructor(private api: Backend, private m: Metrics, private now: () => number = Date.now) {}

  attach(sink: LayoutSink) {
    this.sink = sink;
    if (this.layout) sink.setLayout(this.layout, true);
  }

  async init() {
    const root = await this.api.getRoot();
    this.model = new TreeModel(root.name);
    this.model.ingest(await this.api.listDirectory(""));
    const kids = this.model.children("") ?? [];
    if (root.initial && (await this.reveal(root.initial, true))) return;
    if (kids.length) this.select(kids[entryChildIndex(kids.length)].id);
  }

  private ingest(listings: Listing[]) {
    let changed = false;
    for (const l of listings) changed = this.model.ingest(l) || changed;
    return changed;
  }

  relayout(instant = false) {
    if (!this.selectedId) return;
    this.layout = computeLayout(this.model, this.selectedId, this.m, this.now());
    this.sink?.setLayout(this.layout, instant);
    this.onChange();
  }

  select(id: string, instant = false) {
    if (!this.model.get(id) || id === "") return;
    this.selectedId = id;
    this.lastChild.set(parentOf(id), id);
    this.relayout(instant);
    void this.ensurePreview();
  }

  /** Loads whatever the preview columns need for the current selection, then relayouts. */
  async ensurePreview() {
    for (let round = 0; round < 2; round++) {
      const sel = this.selectedId;
      const missing = this.model.missingForPreview(sel).filter((p) => !this.inflight.has(p));
      if (!missing.length) return;
      missing.forEach((p) => this.inflight.add(p));
      try {
        const listings = await this.api.listDirectories(missing);
        const got = new Set(listings.map((l) => l.path));
        for (const p of missing) if (!got.has(p)) this.model.markUnreadable(p);
        if (this.ingest(listings)) this.relayout();
      } finally {
        missing.forEach((p) => this.inflight.delete(p));
      }
    }
  }

  async reveal(path: string, instant = false): Promise<boolean> {
    try {
      const chain = await this.api.revealPath(path);
      this.ingest(chain);
    } catch {
      return false;
    }
    if (!this.model.get(path)) return false;
    this.select(path, instant);
    return true;
  }

  move(delta: number) {
    const sibs = this.model.siblings(this.selectedId);
    const i = sibs.findIndex((s) => s.id === this.selectedId);
    const j = Math.max(0, Math.min(sibs.length - 1, i + delta));
    if (j !== i) this.select(sibs[j].id);
  }

  async enter() {
    const node = this.model.get(this.selectedId);
    if (!node?.isDirectory) return;
    if (!node.loaded) {
      try {
        this.ingest([await this.api.listDirectory(node.id)]);
      } catch {
        this.model.markUnreadable(node.id);
        return;
      }
    }
    const kids = this.model.children(node.id) ?? [];
    if (!kids.length || this.selectedId !== node.id) return;
    const remembered = this.lastChild.get(node.id);
    const target = remembered && this.model.get(remembered) ? remembered : kids[entryChildIndex(kids.length)].id;
    this.select(target);
  }

  leave() {
    const parent = parentOf(this.selectedId);
    if (parent) this.select(parent);
  }

  async activate() {
    const node = this.model.get(this.selectedId);
    if (!node) return;
    if (node.isDirectory) await this.enter();
    else await this.api.openPath(node.id);
  }

  /** Returns true when the key was handled. */
  key(e: KeyboardEvent): boolean {
    switch (e.key) {
      case "ArrowUp":
        this.move(-1);
        return true;
      case "ArrowDown":
        this.move(1);
        return true;
      case "ArrowRight":
      case " ":
        void this.enter();
        return true;
      case "ArrowLeft":
        this.leave();
        return true;
      case "Enter":
        void this.activate();
        return true;
      case "PageUp":
        this.move(-10);
        return true;
      case "PageDown":
        this.move(10);
        return true;
      case "Home":
        this.move(-Infinity);
        return true;
      case "End":
        this.move(Infinity);
        return true;
    }
    return false;
  }

  wheel(deltaY: number) {
    this.wheelAcc += deltaY;
    const step = 40;
    while (Math.abs(this.wheelAcc) >= step) {
      const dir = Math.sign(this.wheelAcc);
      this.wheelAcc -= dir * step;
      this.move(dir);
    }
  }

  pick(id: string, e: MouseEvent) {
    if (e.type === "dblclick") {
      const n = this.model.get(id);
      if (n && !n.isDirectory) void this.api.openPath(id);
      return;
    }
    this.select(id);
  }

  /** Watcher notification: reload the affected listings we actually hold. */
  async refresh(dirs: string[]) {
    const loaded = [...new Set(dirs)].filter((d) => this.model.get(d)?.loaded);
    if (!loaded.length) return;
    const listings = await this.api.listDirectories(loaded);
    // Folders that vanished come back missing from the batch; drop them via their parent.
    if (!this.ingest(listings)) return;
    let sel = this.selectedId;
    while (sel && !this.model.get(sel)) sel = parentOf(sel);
    if (!sel) sel = this.model.children("")?.[0]?.id ?? "";
    this.selectedId = sel;
    this.relayout();
    void this.ensurePreview();
  }
}
