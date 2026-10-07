import type { ReactNode } from "react";
import { useApp } from "../state/store";

export function ShortcutModal() {
  const closeShortcuts = useApp((s) => s.closeShortcuts);

  return (
    <div
      className="palette-backdrop fixed inset-0 z-50 flex items-start justify-center bg-black/60 p-4 pt-[10vh] backdrop-blur-sm"
      data-testid="shortcut-modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          closeShortcuts();
        }
      }}
    >
      <div
        className="shortcut-modal flex max-h-[80vh] w-full max-w-2xl flex-col overflow-y-auto rounded-xl border border-border bg-popover p-5 shadow-2xl scrollbar-thin"
        data-testid="shortcut-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard Shortcuts"
      >
        <div className="shortcut-modal-head flex items-center justify-between">
          <h2 className="text-base font-semibold">Keyboard Shortcuts</h2>
          <button
            type="button"
            className="rounded-md p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            data-testid="close-shortcuts-icon"
            onClick={closeShortcuts}
            title="Close (Esc)"
          >
            ✕
          </button>
        </div>

        <section className="shortcut-section mt-4">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Navigation &amp; Window</h3>
          <table className="shortcut-table w-full text-sm">
            <tbody>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Command Palette</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Ctrl</Kbd> + <Kbd>K</Kbd> / <Kbd>Cmd</Kbd> + <Kbd>K</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Toggle Sidebar</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Ctrl</Kbd> + <Kbd>B</Kbd> / <Kbd>Cmd</Kbd> + <Kbd>B</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">New Task / Session</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Ctrl</Kbd> + <Kbd>N</Kbd> / <Kbd>Cmd</Kbd> + <Kbd>N</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Show Keyboard Shortcuts</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>?</Kbd> or <Kbd>Ctrl</Kbd> + <Kbd>/</Kbd>
                </td>
              </tr>
              <tr>
                <td className="py-1.5">Close Dialogs / Palettes</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Esc</Kbd>
                </td>
              </tr>
            </tbody>
          </table>
        </section>

        <section className="shortcut-section mt-4">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Session &amp; Composer</h3>
          <table className="shortcut-table w-full text-sm">
            <tbody>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Focus Composer</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Ctrl</Kbd> + <Kbd>L</Kbd> / <Kbd>Cmd</Kbd> + <Kbd>L</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Send Message</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Enter</Kbd> or <Kbd>Ctrl</Kbd> + <Kbd>Enter</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">New Line in Composer</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Shift</Kbd> + <Kbd>Enter</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Toggle Changes (Review Diff)</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Ctrl</Kbd> + <Kbd>Shift</Kbd> + <Kbd>D</Kbd>
                </td>
              </tr>
              <tr>
                <td className="py-1.5">Stop / Interrupt Agent Turn</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Ctrl</Kbd> + <Kbd>Esc</Kbd>
                </td>
              </tr>
            </tbody>
          </table>
        </section>

        <section className="shortcut-section mt-4">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Native Tools &amp; Files</h3>
          <table className="shortcut-table w-full text-sm">
            <tbody>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Open in VS Code</td>
                <td className="py-1.5 text-right text-muted-foreground">Header &amp; Diff toolbar button</td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Open in Cursor</td>
                <td className="py-1.5 text-right text-muted-foreground">Header &amp; Diff toolbar button</td>
              </tr>
              <tr>
                <td className="py-1.5">Reveal in File Manager</td>
                <td className="py-1.5 text-right text-muted-foreground">Header &amp; Diff toolbar button</td>
              </tr>
            </tbody>
          </table>
        </section>

        <div className="shortcut-modal-footer mt-4 flex justify-end">
          <button
            type="button"
            className="rounded-md bg-primary px-4 py-1.5 text-sm font-medium text-primary-foreground hover:bg-primary/90"
            data-testid="close-shortcuts"
            onClick={closeShortcuts}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-border bg-secondary px-1.5 py-0.5 font-mono text-[11px] text-foreground">
      {children}
    </kbd>
  );
}
