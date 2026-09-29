// Orthogonal connector routing. Input is plain geometry (so it can run on interpolated
// positions every animation frame); output is polylines made of H/V segments only.
//
// Preview connectors attach to the child row nearest the parent (straight when the group
// spans the parent row). The vertical run hugs the parent when nothing is in the way,
// otherwise it runs next to the child column in a lane: lanes stack outward so edges that
// travel further sit further left, which keeps nested edges from crossing.
import type { Metrics } from "./metrics";
import type { Layout } from "./layout";

export interface Box {
  x: number;
  y: number; // row top
  w: number;
}

export interface GroupGeom {
  id: string;
  level: number;
  parent: Box;
  first: Box;
  last: Box;
  siblings: Box[]; // other names in the parent's column
  targets: Box[]; // every name in the children's column
  baseX: number; // the column's left edge (a group beside an overhanging name sits right of it)
}

export type Point = [number, number];
export interface Route {
  id: string;
  kind: "path" | "preview";
  points: Point[];
  /** first child row y, used to decide whether the target is on screen */
  spanTop: number;
  spanBottom: number;
}

interface Pending {
  g: GroupGeom;
  xs: number;
  xe: number;
  yp: number;
  yt: number;
  mode: "straight" | "near" | "far";
  xv: number;
  lane: number;
  /** Groups pushed right by an overhanging name: run in the column's lanes, then detour along
   *  the blank row next to the group (always free: groups are stacked one row apart). */
  detour?: { y: number; x2: number; yt: number; xe: number };
  /** Lanes blocked by an overhanging name: drop down beside it and land on the group's edge. */
  landing?: Point[];
}

const EPS = 0.5;

export function routePath(id: string, from: Box, to: Box, m: Metrics): Route {
  const y = from.y + m.connectorY;
  const ty = to.y + m.connectorY;
  const xs = from.x + from.w + m.connGapL;
  const xe = to.x - m.connGapR;
  const points: Point[] = Math.abs(ty - y) < EPS ? [[xs, y], [xe, y]] : [[xs, y], [(xs + xe) / 2, y], [(xs + xe) / 2, ty], [xe, ty]];
  return { id, kind: "path", points, spanTop: to.y, spanBottom: to.y + m.rowH };
}

export function routeGroups(groups: GroupGeom[], m: Metrics, overflow?: Map<number, number>): Route[] {
  const byLevel = new Map<number, Pending[]>();
  for (const g of groups) {
    const yp = g.parent.y + m.connectorY;
    const yFirst = g.first.y + m.connectorY;
    const yLast = g.last.y + m.connectorY;
    const xs = g.parent.x + g.parent.w + m.connGapL;
    const xe = g.first.x - m.connGapR;
    const p: Pending = { g, xs, xe, yp, yt: yp, mode: "straight", xv: xe, lane: 0 };
    // Detour only when the group was pushed by someone else's name (an overhanging parent just
    // routes normally toward its own, shifted, group).
    const shifted = g.first.x > g.baseX + EPS && xs < g.baseX - m.connGapR - m.nearChild;
    if ((yp < yFirst - EPS || yp > yLast + EPS) && shifted) {
      const fromAbove = yp < yFirst;
      const xeFinal = xe;
      p.xe = g.baseX - m.connGapR;
      // Run along the group's outer edge (bottom of the blank row next to it) into its first name.
      p.yt = fromAbove ? g.first.y - m.landGap : g.last.y + m.rowH + m.landGap;
      p.detour = { y: p.yt, x2: xeFinal, yt: p.yt, xe: xeFinal };
      p.mode = "far";
    } else if (yp < yFirst - EPS || yp > yLast + EPS) {
      p.yt = yp < yFirst ? yFirst - m.enterOffset : yLast + m.enterOffset;
      p.xv = xs + m.nearParent;
      const lo = Math.min(p.yp, p.yt);
      const hi = Math.max(p.yp, p.yt);
      // Any name whose glyph band the vertical would touch, and that reaches past it.
      const blocked = g.siblings.some((o) => o.y + 1 < hi && o.y + m.rowH - 1 > lo && o.x + o.w + m.connGapL > p.xv - EPS);
      p.mode = blocked ? "far" : "near";
    }
    if (!byLevel.has(g.level)) byLevel.set(g.level, []);
    byLevel.get(g.level)!.push(p);
  }

  const out: Route[] = [];
  for (const list of byLevel.values()) {
    const span = (p: Pending) => [Math.min(p.yp, p.yt), Math.max(p.yp, p.yt)] as const;
    for (let pass = 0; pass < 8; pass++) {
      let changed = false;
      const demote = (p: Pending) => ((p.mode = "far"), (changed = true));
      // A near run is invalid if another edge's first horizontal passes through it.
      for (const p of list) {
        if (p.mode !== "near") continue;
        const [lo, hi] = span(p);
        if (list.some((q) => q !== p && q.yp > lo + EPS && q.yp < hi - EPS && (q.mode === "near" ? q.xv : q.xe) > p.xv - EPS)) demote(p);
      }
      assignLanes(list.filter((p) => p.mode === "far" && p.yt < p.yp).sort((a, b) => b.yp - a.yp));
      assignLanes(list.filter((p) => p.mode === "far" && p.yt > p.yp).sort((a, b) => a.yp - b.yp));
      for (const p of list) if (p.mode === "far") p.xv = p.xe - m.nearChild - p.lane * m.laneGap;
      // When the parent is the column's widest name, "next to the parent" lands inside the lane
      // zone; a near run to the right of an overlapping lane would cut through it.
      for (const p of list) {
        if (p.mode !== "near") continue;
        const [lo, hi] = span(p);
        if (list.some((q) => q.mode === "far" && q.xv < p.xv + EPS && span(q)[0] < hi - EPS && lo < span(q)[1] - EPS)) demote(p);
      }
      if (!changed) break;
    }
    // Lanes that do not fit between the names they pass and the children. First choice (as in the
    // reference): run down in the free space right of the overhanging name and land on the
    // group's top/bottom edge. Otherwise report how much wider the gap must be (the layout
    // widens it) and clamp meanwhile.
    const needs = new Map<Pending, number>();
    for (const p of list) {
      if (p.mode !== "far" || p.detour) continue;
      const [lo, hi] = span(p);
      let limit = p.xs + m.minRun;
      for (const o of p.g.siblings) if (o.y + 1 < hi && o.y + m.rowH - 1 > lo) limit = Math.max(limit, o.x + o.w + m.connGapL);
      const need = limit - p.xv;
      if (need <= 0) continue;
      const land = landingRoute(p, limit, m);
      if (land) p.landing = land;
      else needs.set(p, need);
    }
    const build = (p: Pending): Point[] => {
      const d = p.detour;
      if (p.mode === "straight") return [[p.xs, p.yp], [p.xe, p.yp]];
      if (p.landing) return p.landing;
      if (d) return [[p.xs, p.yp], [p.xv, p.yp], [p.xv, d.y], [d.xe, d.y]];
      return [[p.xs, p.yp], [p.xv, p.yp], [p.xv, p.yt], [p.xe, p.yt]];
    };
    // A landing that crosses another connector falls back to widening.
    for (const p of list) {
      if (!p.landing) continue;
      const mine: Route = { id: "a", kind: "preview", points: p.landing, spanTop: 0, spanBottom: 0 };
      const others = list.filter((q) => q !== p).map((q) => ({ id: "b", kind: "preview" as const, points: build(q), spanTop: 0, spanBottom: 0 }));
      if (crossings([mine, ...others]).length) {
        p.landing = undefined;
        const [lo, hi] = span(p);
        let limit = p.xs + m.minRun;
        for (const o of p.g.siblings) if (o.y + 1 < hi && o.y + m.rowH - 1 > lo) limit = Math.max(limit, o.x + o.w + m.connGapL);
        needs.set(p, limit - p.xv);
      }
    }
    for (const [p, need] of needs) {
      if (overflow) overflow.set(p.g.level, Math.max(overflow.get(p.g.level) ?? 0, need));
      p.xv += need;
    }
    for (const p of list) {
      const points = build(p);
      out.push({ id: p.g.id, kind: "preview", points, spanTop: p.g.first.y, spanBottom: p.g.last.y + m.rowH });
    }
  }
  return out;
}

/** Vertical just right of the names in the way, ending with a short hook on the group's edge. */
function landingRoute(p: Pending, limit: number, m: Metrics): Point[] | null {
  const g = p.g;
  const x = limit + m.nearParent - m.connGapL;
  const down = p.yt > p.yp;
  const own = g.targets.filter((b) => b.y > g.first.y - EPS && b.y < g.last.y + EPS);
  const end = Math.max(...own.map((b) => b.x + b.w));
  if (!(x > g.first.x + m.charW && x < end - m.charW)) return null;
  const yEdge = down ? g.first.y - m.landGap : g.last.y + m.rowH + m.landGap;
  const pts: Point[] = [[p.xs, p.yp], [x, p.yp], [x, yEdge], [x - m.hook, yEdge]];
  const route: Route = { id: g.id, kind: "preview", points: pts, spanTop: 0, spanBottom: 0 };
  return textHits([route], [...g.siblings, ...g.targets].map((b, i) => ({ id: String(i), ...b })), m).length ? null : pts;
}

function assignLanes(ordered: Pending[]) {
  const done: Pending[] = [];
  for (const p of ordered) {
    const lo = Math.min(p.yp, p.yt);
    const hi = Math.max(p.yp, p.yt);
    let lane = 0;
    for (const q of done) {
      const qlo = Math.min(q.yp, q.yt);
      const qhi = Math.max(q.yp, q.yt);
      if (qlo < hi + EPS && lo < qhi + EPS) lane = Math.max(lane, q.lane + 1);
    }
    p.lane = lane;
    done.push(p);
  }
}

/** SVG path with rounded elbows. */
export function toPathD(points: Point[], radius: number): string {
  if (points.length < 2) return "";
  const f = (v: number) => Math.round(v * 100) / 100;
  let d = `M${f(points[0][0])} ${f(points[0][1])}`;
  for (let i = 1; i < points.length - 1; i++) {
    const [px, py] = points[i - 1];
    const [cx, cy] = points[i];
    const [nx, ny] = points[i + 1];
    const inLen = Math.hypot(cx - px, cy - py);
    const outLen = Math.hypot(nx - cx, ny - cy);
    const r = Math.min(radius, inLen / 2, outLen / 2);
    if (r < 0.01) {
      d += `L${f(cx)} ${f(cy)}`;
      continue;
    }
    const ax = cx - ((cx - px) / inLen) * r;
    const ay = cy - ((cy - py) / inLen) * r;
    const bx = cx + ((nx - cx) / outLen) * r;
    const by = cy + ((ny - cy) / outLen) * r;
    d += `L${f(ax)} ${f(ay)}Q${f(cx)} ${f(cy)} ${f(bx)} ${f(by)}`;
  }
  const [lx, ly] = points[points.length - 1];
  return d + `L${f(lx)} ${f(ly)}`;
}

/** True when every segment is axis-aligned (used by tests). */
export function isOrthogonal(points: Point[]): boolean {
  for (let i = 1; i < points.length; i++) {
    if (Math.abs(points[i][0] - points[i - 1][0]) > 1e-6 && Math.abs(points[i][1] - points[i - 1][1]) > 1e-6) return false;
  }
  return true;
}

/** Routes every connector of a layout, reading (possibly interpolated) boxes through `box`. */
export function routeLayout(layout: Layout, box: (id: string) => Box | null, m: Metrics, overflow?: Map<number, number>): Route[] {
  const routes: Route[] = [];
  for (const e of layout.pathEdges) {
    const a = box(e.from);
    const b = box(e.to);
    if (a && b) routes.push(routePath(`p:${e.from}>${e.to}`, a, b, m));
  }
  const firstPreviewCol = layout.groups.length ? layout.groups[0].col : 0;
  const columnBoxes = new Map<number, { id: string; b: Box }[]>();
  const colBoxes = (col: number) => {
    let list = columnBoxes.get(col);
    if (!list) {
      list = (layout.columns[col]?.ids ?? []).flatMap((id) => {
        const b = box(id);
        return b ? [{ id, b }] : [];
      });
      columnBoxes.set(col, list);
    }
    return list;
  };
  const geoms: GroupGeom[] = [];
  for (const g of layout.groups) {
    const parent = box(g.parentId);
    const first = box(g.firstId);
    const last = box(g.lastId);
    const pn = layout.nodes.get(g.parentId);
    if (!parent || !first || !last || !pn) continue;
    const siblings = colBoxes(pn.col).filter((s) => s.id !== g.parentId).map((s) => s.b);
    const target = layout.nodes.get(g.firstId);
    const baseX = first.x - (target && layout.columns[g.col] ? target.x - layout.columns[g.col].x : 0);
    const targets = colBoxes(g.col).map((s) => s.b);
    geoms.push({ id: `g:${g.parentId}`, level: g.col - firstPreviewCol, parent, first, last, siblings, targets, baseX });
  }
  return routes.concat(routeGroups(geoms, m, overflow));
}

/** Extra horizontal room each preview column needs so its connector lanes fit (by column). */
export function laneOverflow(layout: Layout, m: Metrics): Map<number, number> {
  const byLevel = new Map<number, number>();
  routeLayout(layout, (id) => layout.nodes.get(id) ?? null, m, byLevel);
  const first = layout.groups.length ? layout.groups[0].col : 0;
  return new Map([...byLevel].map(([level, px]) => [first + level, Math.ceil(px)]));
}

/** Pairs of routes whose segments cross (a horizontal strictly through a vertical) or run on
 *  top of each other (collinear overlap). */
export function crossings(routes: Route[]): [string, string][] {
  type Seg = { id: string; h: boolean; a: number; b: number; c: number };
  const segs: Seg[] = [];
  for (const r of routes)
    for (let i = 1; i < r.points.length; i++) {
      const [x1, y1] = r.points[i - 1];
      const [x2, y2] = r.points[i];
      if (y1 === y2) segs.push({ id: r.id, h: true, a: Math.min(x1, x2), b: Math.max(x1, x2), c: y1 });
      else segs.push({ id: r.id, h: false, a: Math.min(y1, y2), b: Math.max(y1, y2), c: x1 });
    }
  const out: [string, string][] = [];
  for (const s of segs)
    for (const t of segs) {
      if (s.id === t.id) continue;
      if (s.h && !t.h && t.c > s.a + 0.5 && t.c < s.b - 0.5 && s.c > t.a + 0.5 && s.c < t.b - 0.5) out.push([s.id, t.id]);
      if (s.h === t.h && s.id < t.id && Math.abs(s.c - t.c) < 1 && Math.min(s.b, t.b) - Math.max(s.a, t.a) > 1) out.push([s.id, t.id]);
    }
  return out;
}
/** Connector segments that run through a name's glyphs (routes vs laid-out text boxes). */
export function textHits(routes: Route[], boxes: { id: string; x: number; y: number; w: number }[], m: Metrics): string[] {
  const out: string[] = [];
  for (const r of routes)
    for (let i = 1; i < r.points.length; i++) {
      const [x1, y1] = r.points[i - 1];
      const [x2, y2] = r.points[i];
      const [ax, bx] = [Math.min(x1, x2), Math.max(x1, x2)];
      const [ay, by] = [Math.min(y1, y2), Math.max(y1, y2)];
      for (const b of boxes) {
        const gx0 = b.x + 1;
        const gx1 = b.x + b.w - 1;
        const gy0 = b.y + 1;
        const gy1 = b.y + m.rowH - 1;
        if (ax < gx1 && bx > gx0 && ay < gy1 && by > gy0) out.push(`${r.id} through "${b.id}"`);
      }
    }
  return out;
}