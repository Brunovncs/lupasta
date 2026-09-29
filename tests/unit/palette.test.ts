import { describe, expect, test } from "bun:test";
import { AGE_MAX_MS, AGE_MIN_MS, ageForT, ageT, classify, colorFor, extensionOf, rampColor } from "../../src/styles/palette";

describe("classification (shared file-kinds.json)", () => {
  test.each([
    ["OrthogonalRouter.swift", false, false, "code"],
    ["build.sh", false, false, "code"],
    ["notes.txt", false, false, "text"],
    ["Orc shift roster.csv", false, false, "text"],
    ["Beacons maintenance schedule.ics", false, false, "text"],
    ["browser-preview.png", false, false, "image"],
    ["Oath renewal form.pdf", false, false, "document"],
    ["Benefits enrollment.xlsx", false, false, "document"],
    ["Treebeard voice memo.m4a", false, false, "binary"],
    ["file-browser-tests", false, false, "binary"],
    [".eye-is-watching.log", false, true, "hidden"],
    ["Sources", true, false, "directory"],
    [".git", true, true, "special"],
    ["README.md", false, false, "special"],
  ] as const)("%s → %s", (name, dir, hidden, kind) => {
    expect(classify(name, dir, hidden)).toBe(kind);
  });
  test("extensions are lowercased; dotfiles have none", () => {
    expect(extensionOf("A.PNG")).toBe("png");
    expect(extensionOf(".gitignore")).toBe("");
    expect(extensionOf("archive.tar.gz")).toBe("gz");
  });
});

describe("recency ramp", () => {
  const now = 2_000_000_000_000;
  test("ageT clamps and ageForT inverts it", () => {
    expect(ageT(now, now)).toBe(0);
    expect(ageT(now - AGE_MIN_MS / 2, now)).toBe(0);
    expect(ageT(now - AGE_MAX_MS * 10, now)).toBe(1);
    for (const t of [0.1, 0.5, 0.9]) expect(ageT(now - ageForT(t), now)).toBeCloseTo(t, 6);
  });
  test("endpoints: white when fresh, saturated hue when old", () => {
    expect(rampColor(0, "path")).toBe("rgb(252,252,252)");
    expect(rampColor(1, "path")).toBe("rgb(240,108,4)");
    expect(rampColor(1, "preview")).toBe("rgb(0,0,236)");
    expect(rampColor(0.5, "preview")).toBe("rgb(130,128,138)");
  });
  test("path and preview hues differ for the same age", () => {
    expect(rampColor(0.8, "path")).not.toBe(rampColor(0.8, "preview"));
  });
  test("kind mode maps kinds to fixed colors", () => {
    expect(colorFor("code", 0.3, "path", "kind")).toBe("#8ab0ff");
    expect(colorFor("code", 0.3, "path", "age")).toBe(rampColor(0.3, "path"));
  });
});
