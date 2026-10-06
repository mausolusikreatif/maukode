import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  focusWindow,
  isDesktop,
  openInEditor,
  revealInFileManager,
  showNotification,
} from "./platform";

describe("platform native helpers", () => {
  beforeEach(() => {
    (globalThis as unknown as { window?: { open: unknown; focus: unknown } }).window = {
      open: vi.fn(),
      focus: vi.fn(),
    };
  });

  afterEach(() => {
    delete (globalThis as unknown as { window?: unknown }).window;
  });

  it("detects whether running in desktop tauri shell", () => {
    expect(isDesktop()).toBe(false);
  });

  it("handles openInEditor fallback in web/browser mode", async () => {
    const openSpy = vi.spyOn(window, "open").mockImplementation(() => null);
    const res = await openInEditor("/path/to/project", "code");
    expect(res.success).toBe(true);
    expect(openSpy).toHaveBeenCalledWith("vscode://file/%2Fpath%2Fto%2Fproject");

    const cursorRes = await openInEditor("/path/to/project", "cursor");
    expect(cursorRes.success).toBe(true);
    expect(openSpy).toHaveBeenCalledWith("cursor://file/%2Fpath%2Fto%2Fproject");
  });

  it("handles revealInFileManager gracefully in browser mode", async () => {
    const res = await revealInFileManager("/path/to/project");
    expect(res.success).toBe(false);
    expect(res.error).toContain("desktop application");
  });

  it("focusWindow calls window.focus", async () => {
    const focusSpy = vi.spyOn(window, "focus").mockImplementation(() => undefined);
    await focusWindow();
    expect(focusSpy).toHaveBeenCalled();
  });

  it("showNotification dispatches with onClick callback", () => {
    const onClick = vi.fn();
    // In node/test environment without global Notification:
    showNotification({ title: "Test", body: "Message", onClick });
    // Does not throw
  });
});
