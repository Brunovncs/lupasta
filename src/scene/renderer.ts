// Imperative scene renderer. Svelte owns the app shell; the scene is updated per animation
// frame by touching only transforms/opacity of pooled elements, and only for what is inside
// the viewport (+ overscan). No framework diffing on the hot path.
import type { Layout } from "./layout";
import { routeLayout, toPathD, type Box, type Route } from "./router";
import { Animator, type Track } from "./animator";
import type { Metrics } from "./metrics";
import { palette } from "../styles/palette";

const SVG_NS = "http://www.w3.org/2000/svg";

/** Writes a label as DOM text (names are untrusted data); every "…" goes into a one-cell span,
 *  since Iosevka's own glyph is two cells wide and the reference's is one. */
function setLabel(el: HTMLElement, label: string) {
  el.dataset.label = label;
  el.replaceChildren();
  label.split("…").forEach((part, i) => {
    if (i > 0) {
      const e = document.createElement("span");
      e.className = "ell";
      e.textContent = "…";
      el.appendChild(e);
    }
    if (part) el.appendChild(document.createTextNode(part));
  });
}

export class SceneRenderer {
  private layout: Layout | null = null;
  private prevLayout: Layout | null = null;
  private anim: Animator;
  private els = new Map<string, HTMLDivElement>();
  private pool: HTMLDivElement[] = [];
  private paths = new Map<string, SVGPathElement>();
  private dots = new Map<string, SVGCircleElement>();
  private nodeLayer: HTMLDivElement;
  private svg: SVGSVGElement;
  private svgGroup: SVGGElement;
  private caret: HTMLDivElement;
  private raf = 0;
  private vw = 0;
  private vh = 0;
  private dpr = 1;
  frames = 0;

  constructor(private host: HTMLElement, private m: Metrics, private onPick: (id: string, e: MouseEvent) => void) {
    this.anim = new Animator(m.duration);
    host.classList.add("scene");
    this.svg = document.createElementNS(SVG_NS, "svg");
    this.svg.classList.add("wires");
    this.svgGroup = document.createElementNS(SVG_NS, "g");
    this.svg.appendChild(this.svgGroup);
    this.caret = document.createElement("div");
    this.caret.className = "caret";
    this.nodeLayer = document.createElement("div");
    this.nodeLayer.className = "nodes";
    this.nodeLayer.appendChild(this.caret);
    host.append(this.svg, this.nodeLayer);
    host.style.setProperty("--font-size", `${m.fontSize}px`);
    host.style.setProperty("--row-h", `${m.rowH}px`);
    host.style.setProperty("--text-y", `${m.textY}px`);
    host.addEventListener("mousedown", (e) => {
      const el = (e.target as HTMLElement).closest<HTMLElement>("[data-id]");
      if (el) this.onPick(el.dataset.id!, e);
    });
    host.addEventListener("dblclick", (e) => {
      const el = (e.target as HTMLElement).closest<HTMLElement>("[data-id]");
      if (el) this.onPick(el.dataset.id!, e);
    });
    this.resize();
  }

  resize() {
    const r = this.host.getBoundingClientRect();
    this.vw = r.width;
    this.vh = r.height;
    this.dpr = window.devicePixelRatio || 1;
    this.svg.setAttribute("width", String(this.vw));
    this.svg.setAttribute("height", String(this.vh));
    if (this.layout) this.setLayout(this.layout, true);
  }

  cameraFor(layout: Layout) {
    const snap = (v: number) => Math.round(v * this.dpr) / this.dpr;
    return {
      x: snap(this.vw / 2 + this.m.cameraBiasX - (layout.minX + layout.maxX) / 2),
      y: snap(this.vh * this.m.focusY),
    };
  }

  setLayout(layout: Layout, instant = false) {
    if (this.layout !== layout) this.prevLayout = this.layout;
    this.layout = layout;
    const d = this.anim.duration;
    if (instant) this.anim.duration = 0;
    this.anim.setTarget(layout, this.cameraFor(layout), performance.now());
    this.anim.duration = d;
    this.draw();
    if (this.anim.running && !this.raf) this.raf = requestAnimationFrame(this.loop);
  }

  get animating() {
    return this.anim.running;
  }

  private loop = (now: number) => {
    this.raf = 0;
    const running = this.anim.tick(now);
    this.frames++;
    this.draw();
    if (running) this.raf = requestAnimationFrame(this.loop);
    else this.prevLayout = null;
  };

  private visible(t: Track, cam: { x: number; y: number }) {
    const o = this.m.overscan;
    const x = t.cur.x + cam.x;
    const y = t.cur.y + cam.y;
    return x + t.node.w > -o && x < this.vw + o && y + this.m.rowH > -o && y < this.vh + o;
  }

  private draw() {
    const cam = this.anim.cam;
    this.nodeLayer.style.transform = `translate(${cam.x}px, ${cam.y}px)`;
    this.svgGroup.setAttribute("transform", `translate(${cam.x} ${cam.y})`);

    const live = new Set<string>();
    for (const [id, t] of this.anim.tracks) {
      if (t.cur.o <= 0.001 || !this.visible(t, cam)) continue;
      live.add(id);
      let el = this.els.get(id);
      if (!el) {
        el = this.pool.pop() ?? document.createElement("div");
        el.className = "node";
        el.dataset.id = id;
        this.els.set(id, el);
        this.nodeLayer.appendChild(el);
      }
      if (el.dataset.label !== t.node.label) setLabel(el, t.node.label);
      if (el.style.color !== t.node.color) el.style.color = t.node.color;
      el.style.transform = `translate(${t.cur.x}px, ${t.cur.y}px)`;
      el.style.opacity = t.cur.o >= 0.999 ? "" : String(t.cur.o);
    }
    for (const [id, el] of this.els) {
      if (live.has(id)) continue;
      el.remove();
      this.els.delete(id);
      this.pool.push(el);
    }

    const sel = this.layout ? this.anim.tracks.get(this.layout.selectedId) : undefined;
    if (sel) {
      this.caret.style.display = "";
      this.caret.style.transform = `translate(${sel.cur.x + this.m.caretX}px, ${sel.cur.y}px)`;
      this.caret.style.width = `${this.m.caretW}px`;
      this.caret.style.height = `${this.m.rowH}px`;
    } else this.caret.style.display = "none";

    this.drawWires(cam);
  }

  private box(id: string): Box | null {
    const t = this.anim.tracks.get(id);
    return t ? { x: t.cur.x, y: t.cur.y, w: t.node.w } : null;
  }

  private routesFor(layout: Layout): Route[] {
    return routeLayout(layout, (id) => this.box(id), this.m);
  }

  private drawWires(cam: { x: number; y: number }) {
    if (!this.layout) return;
    const current = this.routesFor(this.layout);
    const fading = this.prevLayout && this.anim.running ? this.routesFor(this.prevLayout) : [];
    const currentIds = new Set(current.map((r) => r.id));
    const fade = 1 - this.anim.progress(performance.now());
    const seenPaths = new Set<string>();
    const seenDots = new Set<string>();

    const emit = (r: Route, opacity: number) => {
      // Like the reference: a connector whose target lies beyond the screen collapses to a dot.
      const endY = r.points[r.points.length - 1][1] + cam.y;
      const offscreen = r.kind === "preview" && (endY < -this.m.rowH || endY > this.vh + this.m.rowH);
      if (offscreen) {
        let c = this.dots.get(r.id);
        if (!c) {
          c = document.createElementNS(SVG_NS, "circle");
          c.setAttribute("r", String(this.m.dotR));
          c.setAttribute("fill", palette.connectorColor);
          this.svgGroup.appendChild(c);
          this.dots.set(r.id, c);
        }
        c.setAttribute("cx", String(r.points[0][0] + this.m.dotR));
        c.setAttribute("cy", String(r.points[0][1]));
        c.setAttribute("opacity", String(opacity));
        seenDots.add(r.id);
        return;
      }
      let p = this.paths.get(r.id);
      if (!p) {
        p = document.createElementNS(SVG_NS, "path");
        p.setAttribute("fill", "none");
        p.setAttribute("stroke", r.kind === "path" ? palette.pathConnectorColor : palette.connectorColor);
        p.setAttribute("stroke-width", String(r.kind === "path" ? this.m.pathStrokeW : this.m.strokeW));
        this.svgGroup.appendChild(p);
        this.paths.set(r.id, p);
      }
      p.setAttribute("d", toPathD(r.points, this.m.radius));
      p.setAttribute("opacity", String(opacity));
      seenPaths.add(r.id);
    };
    for (const r of current) emit(r, 1);
    for (const r of fading) if (!currentIds.has(r.id)) emit(r, fade);
    for (const [id, p] of this.paths) if (!seenPaths.has(id)) (p.remove(), this.paths.delete(id));
    for (const [id, c] of this.dots) if (!seenDots.has(id)) (c.remove(), this.dots.delete(id));
  }

  destroy() {
    cancelAnimationFrame(this.raf);
    this.host.replaceChildren();
  }
}
