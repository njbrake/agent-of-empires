// @vitest-environment jsdom
//
// Unit coverage for SettingsView's `resolveSelectedProfile` helper. This is
// the post-mount-fetch decision that closes the race where a user-set
// selection would otherwise be silently reverted by the unconditional
// `setSelectedProfile(active.name)` the helper replaced.
//
// The full end-to-end behavior is asserted by
// `web/tests/live/profile-lifecycle.spec.ts`. This test focuses on the
// branch logic.

import { describe, expect, it } from "vitest";
import { resolveSelectedProfile } from "../SettingsView";

describe("resolveSelectedProfile", () => {
  it("preserves the current selection when it still exists in the profile list", () => {
    const profiles = [
      { name: "default", is_default: true },
      { name: "work", is_default: false },
    ];
    expect(resolveSelectedProfile("work", profiles)).toBe("work");
  });

  it("preserves the current selection even when it is the default-flagged profile", () => {
    const profiles = [
      { name: "default", is_default: true },
      { name: "work", is_default: false },
    ];
    expect(resolveSelectedProfile("default", profiles)).toBe("default");
  });

  it("falls back to the default-flagged profile when the current selection was deleted", () => {
    const profiles = [
      { name: "default", is_default: false },
      { name: "work", is_default: true },
    ];
    expect(resolveSelectedProfile("scratch", profiles)).toBe("work");
  });

  it("falls back to the literal 'default' string when neither current nor default-flagged exists", () => {
    const profiles = [{ name: "scratch", is_default: false }];
    expect(resolveSelectedProfile("missing", profiles)).toBe("default");
  });

  it("falls back to 'default' on an empty profile list (boundary)", () => {
    expect(resolveSelectedProfile("anything", [])).toBe("default");
  });
});
