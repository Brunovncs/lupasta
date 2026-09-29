// In-memory filesystem + fake backend for unit tests.
import { FLAG_DIR, TreeModel, type EntryTuple, type Listing } from "../../src/tree/model";
import type { Backend } from "../../src/state/backend";

export type FakeTree = { [name: string]: FakeTree | number }; // number = mtime of a file

export const NOW = 1_800_000_000_000;

function listingOf(tree: FakeTree, path: string): Listing {
  let cur: FakeTree | number = tree;
  for (const p of path.split("/").filter(Boolean)) cur = (cur as FakeTree)[p];
  if (typeof cur === "number" || cur === undefined) throw new Error(`not a dir: ${path}`);
  const entries: EntryTuple[] = Object.keys(cur)
    .sort((a, b) => (a.toLowerCase() < b.toLowerCase() ? -1 : a.toLowerCase() > b.toLowerCase() ? 1 : 0))
    .map((name) => {
      const v = (cur as FakeTree)[name];
      return typeof v === "number" ? [name, name.startsWith(".") ? 2 : 0, v, 1] : [name, FLAG_DIR, NOW - 1000, 0];
    });
  return { path, entries };
}

export function modelFrom(tree: FakeTree, load: string[] = [""]): TreeModel {
  const m = new TreeModel("root");
  for (const p of load) m.ingest(listingOf(tree, p));
  return m;
}

export function fakeBackend(tree: FakeTree, initial: string | null = null, deny = new Set<string>()) {
  const calls: string[] = [];
  const api = {
    getRoot: async () => ({ name: "root", abs: "/root", initial }),
    listDirectory: async (path: string) => {
      calls.push(`list:${path}`);
      if (deny.has(path)) throw new Error("access denied");
      return listingOf(tree, path);
    },
    listDirectories: async (paths: string[]) => {
      calls.push(`batch:${paths.join(",")}`);
      return paths.flatMap((p) => {
        try {
          return deny.has(p) ? [] : [listingOf(tree, p)];
        } catch {
          return [];
        }
      });
    },
    revealPath: async (path: string) => {
      const parts = path.split("/");
      const chain = [""];
      for (let i = 1; i < parts.length; i++) chain.push(parts.slice(0, i).join("/"));
      return chain.map((p) => listingOf(tree, p));
    },
    searchFiles: async () => ({ query: "", hits: [], candidates: 0, strategy: "empty", micros: 0 }),
    indexStatus: async () => ({ state: "ready" as const, indexed: 0, corpus: 0, last_scan_ms: 0 }),
    startIndexing: async () => true,
    openPath: async (p: string) => void calls.push(`open:${p}`),
    onFilesystemChanged: async () => () => {},
    onIndex: async () => [],
  } satisfies Backend;
  return { api: api as Backend, calls };
}

export const REF_TREE: FakeTree = {
  ".localized": 1,
  drcode: {
    Desktop: { "a.png": 1 },
    "file-browser": {
      ".git": { HEAD: 1, config: 1 },
      ".gitignore": 1,
      build: { "a.png": 1, "b.png": 1 },
      "Info.plist": 1,
      "README.md": 1,
      scripts: { "build.sh": 1, "install.sh": 1, "run.sh": 1, "test.sh": 1 },
      Sources: {
        "App.swift": 1, "ConnectorRounding.swift": 1, "FileBrowserModel.swift": 1, "FileBrowserView.swift": 1,
        "NameEditor.swift": 1, "OrthogonalRouter.swift": 1, "ScreenGeometry.swift": 1,
      },
      "temp_0.md": 1,
      "test-folders": {
        "02 Moria": { "West Gate": { "doors.txt": 1 } },
        "05 Mordor": { "Mount Doom": { "Lava.csv": 1 }, "roster.csv": 1 },
        "10 Eagles (Availability Pending)": { "ETA unknown.txt": 1 },
      },
      Tests: { "RouterTests.swift": 1 },
    },
    lupa: {},
  },
  Shared: { "x.txt": 1 },
};
