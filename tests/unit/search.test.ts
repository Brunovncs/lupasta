import { describe, expect, test } from "bun:test";
import { clipDir, normalizeQuery, splitHit } from "../../src/search/query";

describe("search query helpers", () => {
  test("normalization collapses whitespace and separators", () => {
    expect(normalizeQuery("  file   browser ")).toBe("file browser");
    expect(normalizeQuery("file-browser/Sources")).toBe("file-browser Sources");
    expect(normalizeQuery("a\\b")).toBe("a b");
    expect(normalizeQuery("   ")).toBe("");
    expect(normalizeQuery("RTR")).toBe("RTR"); // case is handled (ignored) by the Rust matcher
  });
  test("hits split into parent dir + name", () => {
    expect(splitHit("drcode/file-browser/Sources/App.swift")).toEqual({ dir: "drcode/file-browser/Sources/", name: "App.swift" });
    expect(splitHit("drcode")).toEqual({ dir: "", name: "drcode" });
  });
  test("long dirs keep their tail", () => {
    expect(clipDir("abcdefghij/", 6)).toBe("…ghij/");
    expect(clipDir("short/", 10)).toBe("short/");
  });
});
