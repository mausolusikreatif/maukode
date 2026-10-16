import type { ReactNode } from "react";
import { useApp } from "../state/store";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";
import { Button } from "./ui/button";
import { primaryMod } from "../platform";

export function ShortcutModal() {
  const closeShortcuts = useApp((s) => s.closeShortcuts);
  const mod = primaryMod();

  return (
    <Dialog open onOpenChange={(open) => !open && closeShortcuts()}>
      <DialogContent
        data-testid="shortcut-modal"
        aria-describedby={undefined}
        className="shortcut-modal flex max-h-[80vh] max-w-2xl flex-col gap-0 overflow-y-auto scrollbar-thin p-5"
      >
        <div className="shortcut-modal-head flex items-center justify-between pr-6">
          <DialogTitle>Keyboard Shortcuts</DialogTitle>
        </div>

        <section className="shortcut-section mt-4">
          <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">Navigation &amp; Window</h3>
          <table className="shortcut-table w-full text-sm">
            <tbody>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Command Palette</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>{mod}</Kbd> + <Kbd>K</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Toggle Sidebar</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>{mod}</Kbd> + <Kbd>B</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">New Task / Session</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>{mod}</Kbd> + <Kbd>N</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Show Keyboard Shortcuts</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>?</Kbd> or <Kbd>{mod}</Kbd> + <Kbd>/</Kbd>
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
                  <Kbd>{mod}</Kbd> + <Kbd>L</Kbd>
                </td>
              </tr>
              <tr className="border-b border-border/50">
                <td className="py-1.5">Send Message</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>Enter</Kbd> or <Kbd>{mod}</Kbd> + <Kbd>Enter</Kbd>
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
                  <Kbd>{mod}</Kbd> + <Kbd>Shift</Kbd> + <Kbd>D</Kbd>
                </td>
              </tr>
              <tr>
                <td className="py-1.5">Stop / Interrupt Agent Turn</td>
                <td className="py-1.5 text-right text-muted-foreground">
                  <Kbd>{mod}</Kbd> + <Kbd>Esc</Kbd>
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
          <Button data-testid="close-shortcuts" onClick={closeShortcuts}>
            Close
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-border bg-secondary px-1.5 py-0.5 font-mono text-xs text-foreground">
      {children}
    </kbd>
  );
}
