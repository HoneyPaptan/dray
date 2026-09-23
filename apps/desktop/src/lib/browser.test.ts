import { describe, expect, it } from "vitest";

import { diffInput, modifiersOf, normalizeUrl, pagePoint } from "./browser";

describe("pagePoint", () => {
  const box = { left: 100, top: 50, width: 200, height: 400 };

  it("maps one to one when the frame is drawn at its own size", () => {
    expect(pagePoint({ x: 150, y: 250 }, box, { width: 200, height: 400 })).toEqual({ x: 50, y: 200 });
  });

  it("scales when the frame is drawn smaller than the page", () => {
    // A 400-wide page drawn in a 200-wide box: every drawn pixel is two.
    expect(pagePoint({ x: 150, y: 250 }, box, { width: 400, height: 800 })).toEqual({ x: 100, y: 400 });
  });

  it("clamps a pointer released past the edge onto it", () => {
    expect(pagePoint({ x: 0, y: 900 }, box, { width: 200, height: 400 })).toEqual({ x: 0, y: 400 });
  });

  it("survives a zero-sized box", () => {
    expect(pagePoint({ x: 5, y: 5 }, { ...box, width: 0, height: 0 }, { width: 10, height: 10 })).toEqual({
      x: 0,
      y: 0,
    });
  });
});

describe("modifiersOf", () => {
  it("packs CDP's bitmask", () => {
    expect(modifiersOf({ altKey: true, ctrlKey: false, metaKey: false, shiftKey: true })).toBe(9);
    expect(modifiersOf({ altKey: false, ctrlKey: true, metaKey: true, shiftKey: false })).toBe(6);
  });
});

describe("diffInput", () => {
  it("types what was appended", () => {
    expect(diffInput("hel", "hello")).toEqual({ del: 0, add: "lo" });
  });
  it("deletes what was removed", () => {
    expect(diffInput("hello", "hel")).toEqual({ del: 2, add: "" });
  });
  it("rewrites a corrected word as deletes then keys", () => {
    expect(diffInput("teh", "the")).toEqual({ del: 2, add: "he" });
  });
  it("answers nothing for an unchanged field", () => {
    expect(diffInput("a", "a")).toEqual({ del: 0, add: "" });
  });
});

describe("normalizeUrl", () => {
  it("reads a bare port on localhost as http", () => {
    expect(normalizeUrl("localhost:3000")).toBe("http://localhost:3000");
  });
  it("keeps a scheme as written", () => {
    expect(normalizeUrl("https://example.com/a")).toBe("https://example.com/a");
  });
  it("searches anything that is not a host", () => {
    expect(normalizeUrl("how to center a div")).toMatch(/^https:\/\/www\.google\.com\/search\?q=/);
  });
});
