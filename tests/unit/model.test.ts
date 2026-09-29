import { describe, expect, test } from "bun:test";
import { TreeModel, joinPath, parentOf } from "../../src/tree/model";
import { REF_TREE, modelFrom } from "./helpers";

describe("tree model", () => {
  test("path helpers", () => {
    expect(joinPath("", "a")).toBe("a");
    expect(joinPath("a/b", "c")).toBe("a/b/c");
    expect(parentOf("a/b/c")).toBe("a/b");
    expect(parentOf("a")).toBe("");
  });

  test("ingest builds parent/child relationships", () => {
    const m = modelFrom(REF_TREE, ["", "drcode", "drcode/file-browser"]);
    const fb = m.get("drcode/file-browser")!;
    expect(fb.parentId).toBe("drcode");
    expect(fb.depth).toBe(2);
    expect(fb.loaded).toBe(true);
    expect(m.children("drcode/file-browser")!.map((n) => n.name)).toEqual([
      ".git", ".gitignore", "build", "Info.plist", "README.md", "scripts", "Sources", "temp_0.md", "test-folders", "Tests",
    ]);
    const git = m.get("drcode/file-browser/.git")!;
    expect(git.isDirectory).toBe(true);
    expect(git.type).toBe("special");
    expect(m.get("drcode/file-browser/.gitignore")!.isHidden).toBe(true);
  });

  test("lazy: directories start unloaded, files start loaded", () => {
    const m = modelFrom(REF_TREE, ["", "drcode"]);
    expect(m.get("drcode/file-browser")!.loaded).toBe(false);
    expect(m.children("drcode/file-browser")).toBeNull();
    expect(m.get("drcode/lupa")!.loaded).toBe(false);
    expect(m.get(".localized")!.loaded).toBe(true);
  });

  test("re-ingesting removes vanished entries and their subtrees", () => {
    const m = modelFrom(REF_TREE, ["", "drcode", "drcode/file-browser", "drcode/file-browser/Sources"]);
    expect(m.get("drcode/file-browser/Sources/App.swift")).toBeDefined();
    const v = m.version;
    const changed = m.ingest({ path: "drcode/file-browser", entries: [["README.md", 0, 1, 1]] });
    expect(changed).toBe(true);
    expect(m.version).toBe(v + 1);
    expect(m.get("drcode/file-browser/Sources")).toBeUndefined();
    expect(m.get("drcode/file-browser/Sources/App.swift")).toBeUndefined();
    expect(m.children("drcode/file-browser")!.length).toBe(1);
  });

  test("ingest of an identical listing is a no-op", () => {
    const m = modelFrom(REF_TREE, [""]);
    const v = m.version;
    expect(m.ingest({ path: "", entries: [[".localized", 2, 1, 1], ["drcode", 1, 1_799_999_999_000, 0], ["Shared", 1, 1_799_999_999_000, 0]] })).toBe(false);
    expect(m.version).toBe(v);
  });

  test("file turning into a directory is replaced", () => {
    const m = new TreeModel();
    m.ingest({ path: "", entries: [["x", 0, 1, 1]] });
    m.ingest({ path: "", entries: [["x", 1, 1, 0]] });
    expect(m.get("x")!.isDirectory).toBe(true);
    expect(m.get("x")!.loaded).toBe(false);
  });

  test("pathTo / siblings", () => {
    const m = modelFrom(REF_TREE, ["", "drcode", "drcode/file-browser"]);
    expect(m.pathTo("drcode/file-browser/Tests")).toEqual(["drcode", "drcode/file-browser", "drcode/file-browser/Tests"]);
    expect(m.siblings("drcode").map((s) => s.name)).toEqual([".localized", "drcode", "Shared"]);
  });

  test("missingForPreview asks for sibling dirs and, for a dir, its sub-dirs", () => {
    const m = modelFrom(REF_TREE, ["", "drcode", "drcode/file-browser"]);
    expect(m.missingForPreview("drcode/file-browser/temp_0.md").sort()).toEqual(
      [".git", "build", "scripts", "Sources", "test-folders", "Tests"].map((n) => `drcode/file-browser/${n}`).sort(),
    );
    m.ingest({ path: "drcode/file-browser/test-folders", entries: [["02 Moria", 1, 1, 0], ["x.txt", 0, 1, 1]] });
    expect(m.missingForPreview("drcode/file-browser/test-folders")).toContain("drcode/file-browser/test-folders/02 Moria");
  });
});
