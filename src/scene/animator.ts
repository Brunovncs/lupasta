// Interpolates between successive layouts. Existing nodes slide from wherever they are
// (even mid-flight) to their new slot; new nodes emerge from their parent; removed nodes
// retreat into their parent and fade. The camera eases with the same clock.
import type { Layout, LayoutNode } from "./layout";

export interface Vis {
  x: number;
  y: number;
  o: number;
}

export interface Track {
  node: LayoutNode;
  from: Vis;
  to: Vis;
  cur: Vis;
  exiting: boolean;
}

export const easeOutCubic = (t: number) => 1 - Math.pow(1 - t, 3);
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

export class Animator {
  readonly tracks = new Map<string, Track>();
  cam = { x: 0, y: 0 };
  private camFrom = { x: 0, y: 0 };
  private camTo = { x: 0, y: 0 };
  private start = 0;
  private first = true;
  running = false;

  constructor(public duration: number) {}

  setTarget(layout: Layout, cam: { x: number; y: number }, now: number) {
    const instant = this.first || this.duration <= 0;
    this.first = false;
    for (const ln of layout.nodes.values()) {
      const t = this.tracks.get(ln.id);
      const to = { x: ln.x, y: ln.y, o: 1 };
      if (t) {
        t.node = ln;
        t.from = { ...t.cur };
        t.to = to;
        t.exiting = false;
      } else {
        const parent = this.tracks.get(ln.parentId);
        const from = parent && !parent.exiting ? { x: parent.cur.x, y: parent.cur.y, o: 0 } : { ...to, o: 0 };
        this.tracks.set(ln.id, { node: ln, from, to, cur: { ...from }, exiting: false });
      }
    }
    for (const [id, t] of this.tracks) {
      if (layout.nodes.has(id)) continue;
      const parent = layout.nodes.get(t.node.parentId);
      t.exiting = true;
      t.from = { ...t.cur };
      t.to = parent ? { x: parent.x, y: parent.y, o: 0 } : { ...t.cur, o: 0 };
    }
    this.camFrom = { ...this.cam };
    this.camTo = { ...cam };
    this.start = now;
    this.running = true;
    if (instant) this.tick(now + Math.max(1, this.duration));
  }

  /** Advances to `now`. Returns true while still animating. */
  tick(now: number): boolean {
    const raw = this.duration <= 0 ? 1 : Math.min(1, (now - this.start) / this.duration);
    const e = easeOutCubic(raw);
    for (const [id, t] of this.tracks) {
      t.cur.x = lerp(t.from.x, t.to.x, e);
      t.cur.y = lerp(t.from.y, t.to.y, e);
      t.cur.o = lerp(t.from.o, t.to.o, e);
      if (raw >= 1 && t.exiting) this.tracks.delete(id);
    }
    this.cam.x = lerp(this.camFrom.x, this.camTo.x, e);
    this.cam.y = lerp(this.camFrom.y, this.camTo.y, e);
    this.running = raw < 1;
    return this.running;
  }

  progress(now: number): number {
    return this.duration <= 0 ? 1 : easeOutCubic(Math.min(1, (now - this.start) / this.duration));
  }
}
