// Tree model: a lazily populated mirror of the directories the Rust side has listed.
// Pure data — no layout, no rendering, no IPC.
import { classify, extensionOf, type FileVisualKind } from "../styles/palette";

export const FLAG_DIR = 1;
export const FLAG_HIDDEN = 2;

/** Wire format from `list_directory`: [name, flags, mtimeMs, size]. */
export type EntryTuple = [string, number, number, number];
export interface Listing {
  path: string;
  entries: EntryTuple[];
}

export interface Node {
  id: string; // root-relative path ("" is the root)
  parentId: string | null;
  path: string;
  name: string;
  type: FileVisualKind;
  extension: string;
  isDirectory: boolean;
  isHidden: boolean;
  mtime: number;
  size: number;
  depth: number;
  loaded: boolean;
  children: string[] | null;
}

export const joinPath = (parent: string, name: string) => (parent ? `${parent}/${name}` : name);
export const parentOf = (path: string) => {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
};

export class TreeModel {
  readonly nodes = new Map<string, Node>();
  version = 0;

  constructor(rootName = "") {
    this.nodes.set("", {
      id: "", parentId: null, path: "", name: rootName, type: "directory", extension: "",
      isDirectory: true, isHidden: false, mtime: 0, size: 0, depth: 0, loaded: false, children: null,
    });
  }

  get(id: string): Node | undefined {
    return this.nodes.get(id);
  }

  children(id: string): Node[] | null {
    const n = this.nodes.get(id);
    if (!n?.children) return null;
    return n.children.map((c) => this.nodes.get(c)!);
  }

  /** Replaces the children of `listing.path`. Removed entries drop their whole subtree. */
  ingest(listing: Listing): boolean {
    const parent = this.nodes.get(listing.path);
    if (!parent) return false;
    const next: string[] = [];
    const seen = new Set<string>();
    let changed = !parent.loaded;
    for (const [name, flags, mtime, size] of listing.entries) {
      const id = joinPath(parent.id, name);
      seen.add(id);
      next.push(id);
      const isDirectory = (flags & FLAG_DIR) !== 0;
      const isHidden = (flags & FLAG_HIDDEN) !== 0;
      const old = this.nodes.get(id);
      if (old && old.isDirectory === isDirectory) {
        if (old.mtime !== mtime || old.size !== size || old.isHidden !== isHidden) {
          Object.assign(old, { mtime, size, isHidden, type: classify(name, isDirectory, isHidden) });
          changed = true;
        }
        continue;
      }
      if (old) this.removeSubtree(id);
      changed = true;
      this.nodes.set(id, {
        id, parentId: parent.id, path: id, name, type: classify(name, isDirectory, isHidden),
        extension: isDirectory ? "" : extensionOf(name), isDirectory, isHidden, mtime, size,
        depth: parent.depth + 1, loaded: !isDirectory, children: isDirectory ? null : [],
      });
    }
    for (const old of parent.children ?? []) {
      if (!seen.has(old)) {
        this.removeSubtree(old);
        changed = true;
      }
    }
    if (!changed && parent.children && parent.children.join("\0") !== next.join("\0")) changed = true;
    parent.children = next;
    parent.loaded = true;
    if (changed) this.version++;
    return changed;
  }

  /** A directory the OS refused to list (permissions, dangling junction): treat as empty so it
   *  is not requested again on every selection change. */
  markUnreadable(id: string) {
    const n = this.nodes.get(id);
    if (!n || n.loaded) return;
    n.loaded = true;
    n.children = [];
    this.version++;
  }

  private removeSubtree(id: string) {
    const n = this.nodes.get(id);
    if (!n) return;
    for (const c of n.children ?? []) this.removeSubtree(c);
    this.nodes.delete(id);
  }

  /** Ids from the top-level ancestor down to `id` (root excluded). */
  pathTo(id: string): string[] {
    const out: string[] = [];
    let cur = this.nodes.get(id);
    while (cur && cur.parentId !== null) {
      out.push(cur.id);
      cur = this.nodes.get(cur.parentId);
    }
    return out.reverse();
  }

  siblings(id: string): Node[] {
    const n = this.nodes.get(id);
    if (!n || n.parentId === null) return [];
    return this.children(n.parentId) ?? [];
  }

  /** Directories whose listing the preview columns need but that are not loaded yet. */
  missingForPreview(selectedId: string): string[] {
    const sel = this.nodes.get(selectedId);
    if (!sel) return [];
    const want = this.siblings(selectedId).filter((s) => s.isDirectory);
    if (sel.isDirectory && sel.loaded) want.push(...(this.children(sel.id) ?? []).filter((c) => c.isDirectory));
    return want.filter((n) => !n.loaded).map((n) => n.id);
  }
}
