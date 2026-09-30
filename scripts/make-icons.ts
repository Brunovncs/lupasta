// Generates every app icon from the design below: `bun scripts/make-icons.ts`.
//
// The icon is a piece of the app itself: the path arriving from the left (orange), the
// selection block (#e60000) behind an Iosevka "l", and preview connectors (#8ab0ff, rounded
// orthogonal elbow) leading to names coloured along the age ramp (white → grey → violet).
// Large sizes come from a vector master; 16–48 px are drawn on the pixel grid by hand, since a
// downscaled master turns into mush there. Rasterised with a local Chromium (Edge, Chrome or
// Brave; override with BROWSER=path), then packed into .ico and .icns without extra tools.
import { chromium } from "playwright-core";
import { existsSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const OUT = resolve(import.meta.dir, "../src-tauri/icons");

const C = { red: "#e60000", orange: "#f06c04", blue: "#8ab0ff", white: "#fcfcfc", grey: "#82808a", violet: "#5c50a8" };

// Iosevka Medium "l", font units (1000/em), baseline at y = 0.
const GLYPH_L = "M425 0L75 0L75-80L212-80L212-655L89-655L89-735L302-735L302-80L425-80Z";

/** The design, in the coordinates of an 824-unit tile (Apple's icon grid at 1024). */
const CONTENT = `
  <path d="M0 344H168" stroke="${C.orange}" stroke-width="18" fill="none"/>
  <rect x="188" y="252" width="131" height="200" fill="${C.red}"/>
  <path transform="translate(194 436) scale(0.25)" d="${GLYPH_L}" fill="${C.white}"/>
  <path d="M346 344H444M346 344H374a60 60 0 0 1 60 60V558a60 60 0 0 0 60 60H510" stroke="${C.blue}" stroke-width="18" fill="none"/>
  <rect x="464" y="178" width="250" height="52" fill="${C.white}"/>
  <rect x="464" y="318" width="190" height="52" fill="${C.grey}"/>
  <rect x="530" y="592" width="220" height="52" fill="${C.violet}"/>`;

/** Vector master: `inset` is the tile's margin inside the 1024 canvas. */
function master(inset: number): string {
  const size = 1024 - 2 * inset;
  const k = size / 824;
  const rx = 185 * k;
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1024 1024">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#131217"/><stop offset="1" stop-color="#000"/></linearGradient>
    <clipPath id="tile"><rect x="${inset}" y="${inset}" width="${size}" height="${size}" rx="${rx}"/></clipPath>
  </defs>
  <rect x="${inset}" y="${inset}" width="${size}" height="${size}" rx="${rx}" fill="url(#bg)"/>
  <g clip-path="url(#tile)"><g transform="translate(${inset} ${inset}) scale(${k})">${CONTENT}</g></g>
  <rect x="${inset + 1}" y="${inset + 1}" width="${size - 2}" height="${size - 2}" rx="${rx - 1}" fill="none" stroke="#fff" stroke-opacity="0.07" stroke-width="2"/>
</svg>`;
}

const tile = (n: number, rx: number) =>
  `<rect x="0.5" y="0.5" width="${n - 1}" height="${n - 1}" rx="${rx}" fill="#08070a" stroke="#fff" stroke-opacity="0.1"/>`;

/** 32 px, on the pixel grid. The elbow is square here: a 2 px rounded corner reads as a blob. */
const SMALL = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
  ${tile(32, 6.5)}
  <g shape-rendering="crispEdges">
    <rect x="1" y="14" width="5" height="2" fill="${C.orange}"/>
    <rect x="7" y="10" width="7" height="11" fill="${C.red}"/>
    <rect x="8" y="11" width="4" height="1" fill="${C.white}"/>
    <rect x="10" y="11" width="2" height="9" fill="${C.white}"/>
    <rect x="8" y="19" width="5" height="1" fill="${C.white}"/>
    <rect x="15" y="14" width="2" height="2" fill="${C.blue}"/>
    <rect x="15" y="16" width="2" height="7" fill="${C.blue}"/>
    <rect x="17" y="21" width="2" height="2" fill="${C.blue}"/>
    <rect x="18" y="9" width="9" height="2" fill="${C.white}"/>
    <rect x="18" y="14" width="7" height="2" fill="${C.grey}"/>
    <rect x="20" y="21" width="9" height="2" fill="${C.violet}"/>
  </g>
</svg>`;

/** 24 px, its own grid (scaling the 32 px drawing by 0.75 blurs every edge). */
const SMALL24 = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">
  ${tile(24, 5)}
  <g shape-rendering="crispEdges">
    <rect x="1" y="10" width="3" height="2" fill="${C.orange}"/>
    <rect x="5" y="7" width="5" height="9" fill="${C.red}"/>
    <rect x="6" y="8" width="2" height="1" fill="${C.white}"/>
    <rect x="7" y="8" width="1" height="7" fill="${C.white}"/>
    <rect x="6" y="14" width="3" height="1" fill="${C.white}"/>
    <rect x="11" y="10" width="2" height="6" fill="${C.blue}"/>
    <rect x="13" y="15" width="1" height="2" fill="${C.blue}"/>
    <rect x="11" y="16" width="2" height="1" fill="${C.blue}"/>
    <rect x="14" y="6" width="7" height="2" fill="${C.white}"/>
    <rect x="14" y="10" width="5" height="2" fill="${C.grey}"/>
    <rect x="15" y="15" width="6" height="2" fill="${C.violet}"/>
  </g>
</svg>`;

/** 16 px: no elbow, one pixel per stroke of the glyph. */
const TINY = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">
  ${tile(16, 3.5)}
  <g shape-rendering="crispEdges">
    <rect x="1" y="7" width="2" height="2" fill="${C.orange}"/>
    <rect x="3" y="4" width="4" height="8" fill="${C.red}"/>
    <rect x="4" y="5" width="2" height="1" fill="${C.white}"/>
    <rect x="5" y="5" width="1" height="6" fill="${C.white}"/>
    <rect x="4" y="10" width="3" height="1" fill="${C.white}"/>
    <rect x="8" y="7" width="2" height="2" fill="${C.blue}"/>
    <rect x="11" y="4" width="4" height="2" fill="${C.white}"/>
    <rect x="11" y="7" width="3" height="2" fill="${C.grey}"/>
    <rect x="11" y="10" width="4" height="2" fill="${C.violet}"/>
  </g>
</svg>`;

// Full-bleed-ish tile for Windows and Linux, Apple's grid (824 of 1024) for macOS.
const FULL = master(32);
const APPLE = master(100);

function source(size: number, apple = false): string {
  if (size <= 16) return TINY;
  if (size <= 24) return SMALL24;
  if (size <= 32) return SMALL;
  if (size === 48) return TINY; // exactly 3x the 16 px grid; the master's strokes fade at 48
  return apple ? APPLE : FULL;
}

function findBrowser(): string {
  const candidates = [
    process.env.BROWSER,
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
    "C:/Program Files/Google/Chrome/Application/chrome.exe",
    "C:/Program Files/BraveSoftware/Brave-Browser/Application/brave.exe",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    "/usr/bin/chromium",
    "/usr/bin/google-chrome",
  ];
  const found = candidates.find((c) => c && existsSync(c));
  if (!found) throw new Error("no Chromium found; set BROWSER=/path/to/chrome");
  return found;
}

const browser = await chromium.launch({ executablePath: findBrowser() });
// One fixed viewport, clipped per size: resizing it down to 16 px makes captures fail.
const page = await browser.newPage({ viewport: { width: 1024, height: 1024 }, deviceScaleFactor: 1 });

async function render(svg: string, size: number): Promise<Buffer> {
  const src = `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;
  await page.setContent(`<body style="margin:0;background:transparent"><img src="${src}" width="${size}" height="${size}" style="display:block"></body>`);
  await page.waitForFunction(() => document.images[0]?.complete);
  return page.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } });
}

/** ICO with PNG payloads (Windows Vista and later). */
function ico(images: [number, Buffer][]): Buffer {
  const header = Buffer.alloc(6 + 16 * images.length);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(images.length, 4);
  let offset = header.length;
  images.forEach(([size, png], i) => {
    const e = 6 + 16 * i;
    header.writeUInt8(size >= 256 ? 0 : size, e);
    header.writeUInt8(size >= 256 ? 0 : size, e + 1);
    header.writeUInt16LE(1, e + 4); // planes
    header.writeUInt16LE(32, e + 6); // bpp
    header.writeUInt32LE(png.length, e + 8);
    header.writeUInt32LE(offset, e + 12);
    offset += png.length;
  });
  return Buffer.concat([header, ...images.map(([, png]) => png)]);
}

/** ICNS with PNG payloads (macOS 10.7 and later). */
function icns(chunks: [string, Buffer][]): Buffer {
  const parts = chunks.map(([type, png]) => {
    const h = Buffer.alloc(8);
    h.write(type, 0, "ascii");
    h.writeUInt32BE(png.length + 8, 4);
    return Buffer.concat([h, png]);
  });
  const body = Buffer.concat(parts);
  const h = Buffer.alloc(8);
  h.write("icns", 0, "ascii");
  h.writeUInt32BE(body.length + 8, 4);
  return Buffer.concat([h, body]);
}

const png = new Map<string, Buffer>();
const get = async (size: number, apple = false) => {
  const key = `${size}${apple ? "a" : ""}`;
  if (!png.has(key)) png.set(key, await render(source(size, apple), size));
  return png.get(key)!;
};

const files: [string, Buffer][] = [
  ["32x32.png", await get(32)],
  ["128x128.png", await get(128)],
  ["128x128@2x.png", await get(256)],
  ["icon.png", await get(512)],
];
for (const [name, data] of files) writeFileSync(join(OUT, name), data);

const icoSizes = [16, 24, 32, 48, 64, 128, 256];
writeFileSync(join(OUT, "icon.ico"), ico(await Promise.all(icoSizes.map(async (s) => [s, await get(s)] as [number, Buffer]))));

const icnsTypes: [string, number][] = [
  ["icp4", 16],
  ["icp5", 32],
  ["ic11", 32],
  ["ic12", 64],
  ["ic07", 128],
  ["ic13", 256],
  ["ic08", 256],
  ["ic14", 512],
  ["ic09", 512],
  ["ic10", 1024],
];
const icnsChunks: [string, Buffer][] = [];
for (const [type, size] of icnsTypes) icnsChunks.push([type, await get(size, true)]);
writeFileSync(join(OUT, "icon.icns"), icns(icnsChunks));

writeFileSync(join(OUT, "icon.svg"), FULL);
writeFileSync(join(OUT, "icon-macos.svg"), APPLE);
writeFileSync(join(OUT, "icon-32.svg"), SMALL);
writeFileSync(join(OUT, "icon-24.svg"), SMALL24);
writeFileSync(join(OUT, "icon-16.svg"), TINY);

await browser.close();
console.log(`icons written to ${OUT}`);
