// Decision-matrix tests for the cockpit composer's Enter handler
// added in #1129. The pure helper lets us exercise every cell of the
// matrix (mobile/desktop x idle/mid-turn x modifiers x IME compose)
// without mounting the whole composer + assistant-ui runtime.

import { describe, expect, it } from "vitest";

import { decideEnterAction } from "./Composer";

const plainEnter = {
  key: "Enter",
  shiftKey: false,
  ctrlKey: false,
  metaKey: false,
  isComposing: false,
};

describe("decideEnterAction (#1129)", () => {
  it("returns 'default' for non-Enter keys regardless of context", () => {
    expect(
      decideEnterAction(
        { ...plainEnter, key: "a" },
        { isMobile: true, turnActive: false },
      ),
    ).toBe("default");
    expect(
      decideEnterAction(
        { ...plainEnter, key: "Tab" },
        { isMobile: false, turnActive: true },
      ),
    ).toBe("default");
  });

  it("returns 'default' during IME composition", () => {
    expect(
      decideEnterAction(
        { ...plainEnter, isComposing: true },
        { isMobile: true, turnActive: false },
      ),
    ).toBe("default");
    expect(
      decideEnterAction(
        { ...plainEnter, isComposing: true },
        { isMobile: false, turnActive: true },
      ),
    ).toBe("default");
  });

  it("returns 'default' for Shift/Ctrl/Meta+Enter (modifier passes through)", () => {
    for (const mod of [
      { shiftKey: true },
      { ctrlKey: true },
      { metaKey: true },
    ]) {
      expect(
        decideEnterAction(
          { ...plainEnter, ...mod },
          { isMobile: true, turnActive: false },
        ),
      ).toBe("default");
      expect(
        decideEnterAction(
          { ...plainEnter, ...mod },
          { isMobile: false, turnActive: true },
        ),
      ).toBe("default");
    }
  });

  it("mobile + plain Enter -> 'newline' (idle and mid-turn alike)", () => {
    expect(
      decideEnterAction(plainEnter, { isMobile: true, turnActive: false }),
    ).toBe("newline");
    expect(
      decideEnterAction(plainEnter, { isMobile: true, turnActive: true }),
    ).toBe("newline");
  });

  it("desktop + mid-turn + plain Enter -> 'send' (queue path)", () => {
    expect(
      decideEnterAction(plainEnter, { isMobile: false, turnActive: true }),
    ).toBe("send");
  });

  it("desktop + idle + plain Enter -> 'default' (primitive handles Send)", () => {
    expect(
      decideEnterAction(plainEnter, { isMobile: false, turnActive: false }),
    ).toBe("default");
  });
});
