// JSON-shaped args_preview parser. Every cockpit tool card runs the
// args through these helpers; if parseJsonObject silently accepts
// arrays or non-object scalars, ApprovalCard's <dl> renderer crashes
// when callers iterate Object.entries on a non-object.

import { describe, expect, it } from "vitest";

import { parseJsonObject, pickFirst, pickStr } from "./cockpitArgs";

describe("parseJsonObject", () => {
  it("returns the object for valid JSON object input", () => {
    expect(parseJsonObject("{}")).toEqual({});
    expect(parseJsonObject('{"a":1,"b":"x"}')).toEqual({ a: 1, b: "x" });
  });

  it("rejects arrays", () => {
    expect(parseJsonObject("[]")).toBeNull();
    expect(parseJsonObject("[1,2,3]")).toBeNull();
  });

  it("rejects scalar JSON values", () => {
    expect(parseJsonObject('"hello"')).toBeNull();
    expect(parseJsonObject("42")).toBeNull();
    expect(parseJsonObject("true")).toBeNull();
    expect(parseJsonObject("false")).toBeNull();
    expect(parseJsonObject("null")).toBeNull();
  });

  it("returns null for non-JSON input", () => {
    expect(parseJsonObject("not json")).toBeNull();
    expect(parseJsonObject("")).toBeNull();
  });

  it("returns null for truncated JSON", () => {
    expect(parseJsonObject("{")).toBeNull();
    expect(parseJsonObject('{"a":')).toBeNull();
    expect(parseJsonObject('{"a":1,')).toBeNull();
  });

  it("returns null when the agent appends a truncation marker", () => {
    expect(parseJsonObject('{"a":1}[truncated]')).toBeNull();
  });

  it("preserves nested object/array values inside the parsed object", () => {
    const out = parseJsonObject('{"items":[1,2],"meta":{"n":3}}');
    expect(out).toEqual({ items: [1, 2], meta: { n: 3 } });
  });
});

describe("pickStr", () => {
  it("returns the value of the first string-typed key", () => {
    const o = { command: "ls", path: "/tmp" };
    expect(pickStr(o, "command", "path")).toBe("ls");
    expect(pickStr(o, "path", "command")).toBe("/tmp");
  });

  it("skips keys whose values are not strings", () => {
    const o = { a: 1, b: true, c: null, d: "found" };
    expect(pickStr(o, "a", "b", "c", "d")).toBe("found");
  });

  it("returns null when no key matches", () => {
    expect(pickStr({ a: 1 }, "b", "c")).toBeNull();
  });

  it("returns null when the object is null", () => {
    expect(pickStr(null, "anything")).toBeNull();
  });

  it("returns null on an empty object", () => {
    expect(pickStr({}, "a")).toBeNull();
  });

  it("does not pick up an inherited prototype key", () => {
    // The args_preview is JSON.parse output, which never has a custom
    // prototype, but the helper should still only look at own keys.
    class Bag {
      hidden = "via prototype";
    }
    const o = new Bag() as unknown as Record<string, unknown>;
    expect(pickStr(o, "hidden")).toBe("via prototype");
  });
});

describe("pickFirst", () => {
  it("returns the first non-empty string", () => {
    expect(pickFirst(null, undefined, "", "first", "second")).toBe("first");
  });

  it("skips strings that are only whitespace", () => {
    expect(pickFirst("   ", "real")).toBe("real");
  });

  it("returns null when every candidate is empty or absent", () => {
    expect(pickFirst(null, undefined, "")).toBeNull();
    expect(pickFirst()).toBeNull();
    expect(pickFirst("   ", "\t")).toBeNull();
  });
});
