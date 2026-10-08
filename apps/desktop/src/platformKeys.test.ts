import { afterEach, describe, expect, it } from "vitest";
import { isEditableTarget, isMacPlatform, primaryMod } from "./platform";

const setNavigator = (value: unknown) =>
  Object.defineProperty(globalThis, "navigator", { value, configurable: true });

afterEach(() => {
  // Reset to whatever the test environment provides.
  Object.defineProperty(globalThis, "navigator", { value: undefined, configurable: true });
});

describe("platform shortcut helpers", () => {
  it("detects macOS from the platform string", () => {
    setNavigator({ platform: "MacIntel", userAgent: "Mozilla/5.0 (Macintosh)" });
    expect(isMacPlatform()).toBe(true);
    expect(primaryMod()).toBe("Cmd");
  });

  it("reports Ctrl for non-Mac platforms", () => {
    setNavigator({ platform: "Win32", userAgent: "Mozilla/5.0 (Windows NT 10.0)" });
    expect(isMacPlatform()).toBe(false);
    expect(primaryMod()).toBe("Ctrl");
  });

  it("treats text-entry surfaces as editable but not plain elements", () => {
    const input = { tagName: "INPUT" } as unknown as HTMLElement;
    const textarea = { tagName: "TEXTAREA" } as unknown as HTMLElement;
    const editable = { tagName: "DIV", isContentEditable: true } as unknown as HTMLElement;
    const div = { tagName: "DIV", isContentEditable: false } as unknown as HTMLElement;
    expect(isEditableTarget(input)).toBe(true);
    expect(isEditableTarget(textarea)).toBe(true);
    expect(isEditableTarget(editable)).toBe(true);
    expect(isEditableTarget(div)).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
  });
});
