// @vitest-environment jsdom
//
// Keyboard-affordance tests for DeleteSessionDialog. The dialog opens from
// the workspace sidebar right-click menu; pressing Enter inside it should
// confirm the delete without forcing the user to mouse over to the button
// (issue #1260). Escape continues to cancel, and Enter should not fire a
// second confirm while one is already in flight.

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

import { DeleteSessionDialog } from "../DeleteSessionDialog";
import type { CleanupDefaults } from "../../lib/types";

const cleanupDefaults: CleanupDefaults = {
  delete_worktree: true,
  delete_branch: false,
  delete_sandbox: false,
};

function setup(overrides?: {
  onConfirm?: () => Promise<void>;
  onCancel?: () => void;
  hasManagedWorktree?: boolean;
  isSandboxed?: boolean;
}) {
  const onConfirm = overrides?.onConfirm ?? vi.fn().mockResolvedValue(undefined);
  const onCancel = overrides?.onCancel ?? vi.fn();
  const utils = render(
    <DeleteSessionDialog
      sessionTitle="my-session"
      branchName="feature/foo"
      hasManagedWorktree={overrides?.hasManagedWorktree ?? true}
      isSandboxed={overrides?.isSandboxed ?? false}
      cleanupDefaults={cleanupDefaults}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />,
  );
  return { ...utils, onConfirm, onCancel };
}

afterEach(() => {
  cleanup();
});

describe("DeleteSessionDialog keyboard affordances", () => {
  it("focuses the Delete button on mount so Enter activates it natively", () => {
    const { container } = setup();
    const deleteBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Delete") && !b.textContent.includes("Deleting"),
    );
    expect(deleteBtn).toBeTruthy();
    expect(document.activeElement).toBe(deleteBtn);
  });

  it("Enter pressed inside the dialog calls onConfirm", async () => {
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    setup({ onConfirm });
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onConfirm).toHaveBeenCalledTimes(1);
    expect(onConfirm).toHaveBeenCalledWith({
      delete_worktree: true,
      delete_branch: false,
      delete_sandbox: false,
      force_delete: false,
    });
  });

  it("Escape pressed inside the dialog calls onCancel", () => {
    const onCancel = vi.fn();
    setup({ onCancel });
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("Enter does not fire onConfirm a second time while delete is in flight", async () => {
    // Keep the first confirm promise pending so the component stays in the
    // "deleting" state; a second Enter should be ignored.
    let resolveConfirm: (() => void) | null = null;
    const onConfirm = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveConfirm = () => resolve();
        }),
    );
    setup({ onConfirm });
    fireEvent.keyDown(document, { key: "Enter" });
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onConfirm).toHaveBeenCalledTimes(1);
    resolveConfirm?.();
  });

  it("Enter while focus is on the Delete button does not double-fire onConfirm", () => {
    // When the Delete button is focused (the default on mount), the
    // browser already activates the button on Enter via a synthetic
    // click. The document-level keydown handler must skip Enter when
    // the event target is a button, or onConfirm would be called twice.
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const { container } = setup({ onConfirm });
    const deleteBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Delete") && !b.textContent.includes("Deleting"),
    )!;
    expect(document.activeElement).toBe(deleteBtn);
    // Dispatch keydown from the focused button (bubbles up to document)
    // and the native button activation (click) that the browser would emit.
    fireEvent.keyDown(deleteBtn, { key: "Enter" });
    fireEvent.click(deleteBtn);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("Enter while focus is on the Cancel button cancels rather than confirms", () => {
    const onConfirm = vi.fn().mockResolvedValue(undefined);
    const onCancel = vi.fn();
    const { container } = setup({ onConfirm, onCancel });
    const cancelBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Cancel",
    );
    expect(cancelBtn).toBeTruthy();
    cancelBtn!.focus();
    // The keydown handler should skip Enter when focus is on a non-confirm
    // button, leaving the browser's native button-Enter behavior to drive
    // the Cancel click. Simulate that click here.
    fireEvent.keyDown(cancelBtn!, { key: "Enter" });
    fireEvent.click(cancelBtn!);
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it("dialog has role=dialog, aria-modal, and aria-labelledby pointing at the title", () => {
    const { container } = setup();
    const dialog = container.querySelector('[role="dialog"]');
    expect(dialog).toBeTruthy();
    expect(dialog?.getAttribute("aria-modal")).toBe("true");
    const labelId = dialog?.getAttribute("aria-labelledby");
    expect(labelId).toBeTruthy();
    const titleEl = container.querySelector(`#${labelId}`);
    expect(titleEl?.textContent).toMatch(/Delete Session/);
  });

  it("restores focus to the previously focused element when the dialog unmounts", () => {
    // Create a trigger button outside the dialog and focus it before mount,
    // mirroring how the sidebar context-menu item is focused when the user
    // chooses Delete. After the dialog unmounts, focus should return there.
    const trigger = document.createElement("button");
    trigger.textContent = "trigger";
    document.body.appendChild(trigger);
    trigger.focus();
    expect(document.activeElement).toBe(trigger);

    const { unmount } = setup();
    // Dialog mount steals focus to the Delete button.
    expect(document.activeElement).not.toBe(trigger);
    unmount();
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });
});
