// Screenshot comparison helpers (pngjs + pixelmatch).
import { PNG } from "pngjs";
import pixelmatch from "pixelmatch";
import { readFileSync, writeFileSync } from "node:fs";

export const readPng = (p: string) => PNG.sync.read(readFileSync(p));

/** Pixel diff ratio between two same-size PNGs; writes a diff image when `out` is given. */
export function diffRatio(a: PNG, b: PNG, out?: string, threshold = 0.15): number {
  const diff = new PNG({ width: a.width, height: a.height });
  const n = pixelmatch(a.data, b.data, diff.data, a.width, a.height, { threshold, includeAA: false });
  if (out) writeFileSync(out, PNG.sync.write(diff));
  return n / (a.width * a.height);
}

/**
 * Overlay for eyeballing alignment against the reference: red = reference only,
 * cyan = ours only, white/gray = both agree.
 */
export function overlay(ref: PNG, cur: PNG, out: string) {
  const o = new PNG({ width: ref.width, height: ref.height });
  const lum = (d: Buffer, i: number) => 0.3 * d[i] + 0.59 * d[i + 1] + 0.11 * d[i + 2];
  for (let i = 0; i < o.data.length; i += 4) {
    const r = lum(ref.data, i);
    const c = lum(cur.data, i);
    o.data[i] = r;
    o.data[i + 1] = c;
    o.data[i + 2] = c;
    o.data[i + 3] = 255;
  }
  writeFileSync(out, PNG.sync.write(o));
}

/** Fraction of "ink" pixels (non-black) that land on ink in the other image, both ways. */
export function inkAgreement(ref: PNG, cur: PNG, level = 60): number {
  let both = 0;
  let either = 0;
  for (let i = 0; i < ref.data.length; i += 4) {
    const a = ref.data[i] + ref.data[i + 1] + ref.data[i + 2] > level * 3;
    const b = cur.data[i] + cur.data[i + 1] + cur.data[i + 2] > level * 3;
    if (a || b) either++;
    if (a && b) both++;
  }
  return either ? both / either : 1;
}
