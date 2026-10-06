import { useApp } from "../state/store";

export function ShortcutModal() {
  const closeShortcuts = useApp((s) => s.closeShortcuts);

  return (
    <div
      className="palette-backdrop"
      data-testid="shortcut-modal-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          closeShortcuts();
        }
      }}
    >
      <div
        className="shortcut-modal"
        data-testid="shortcut-modal"
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard Shortcuts"
      >
        <div className="shortcut-modal-head">
          <h2>Keyboard Shortcuts</h2>
          <button
            type="button"
            className="link icon-btn"
            data-testid="close-shortcuts-icon"
            onClick={closeShortcuts}
            title="Close (Esc)"
          >
            ✕
          </button>
        </div>

        <section className="shortcut-section">
          <h3>Navigation & Window</h3>
          <table className="shortcut-table">
            <tbody>
              <tr>
                <td>Command Palette</td>
                <td>
                  <kbd>Ctrl</kbd> + <kbd>K</kbd> / <kbd>Cmd</kbd> + <kbd>K</kbd>
                </td>
              </tr>
              <tr>
                <td>Toggle Sidebar</td>
                <td>
                  <kbd>Ctrl</kbd> + <kbd>B</kbd> / <kbd>Cmd</kbd> + <kbd>B</kbd>
                </td>
              </tr>
              <tr>
                <td>New Task / Session</td>
                <td>
                  <kbd>Ctrl</kbd> + <kbd>N</kbd> / <kbd>Cmd</kbd> + <kbd>N</kbd>
                </td>
              </tr>
              <tr>
                <td>Show Keyboard Shortcuts</td>
                <td>
                  <kbd>?</kbd> or <kbd>Ctrl</kbd> + <kbd>/</kbd>
                </td>
              </tr>
              <tr>
                <td>Close Dialogs / Palettes</td>
                <td>
                  <kbd>Esc</kbd>
                </td>
              </tr>
            </tbody>
          </table>
        </section>

        <section className="shortcut-section">
          <h3>Session & Composer</h3>
          <table className="shortcut-table">
            <tbody>
              <tr>
                <td>Focus Composer</td>
                <td>
                  <kbd>Ctrl</kbd> + <kbd>L</kbd> / <kbd>Cmd</kbd> + <kbd>L</kbd>
                </td>
              </tr>
              <tr>
                <td>Send Message</td>
                <td>
                  <kbd>Enter</kbd> or <kbd>Ctrl</kbd> + <kbd>Enter</kbd>
                </td>
              </tr>
              <tr>
                <td>New Line in Composer</td>
                <td>
                  <kbd>Shift</kbd> + <kbd>Enter</kbd>
                </td>
              </tr>
              <tr>
                <td>Toggle Changes (Review Diff)</td>
                <td>
                  <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>D</kbd>
                </td>
              </tr>
              <tr>
                <td>Stop / Interrupt Agent Turn</td>
                <td>
                  <kbd>Ctrl</kbd> + <kbd>Esc</kbd>
                </td>
              </tr>
            </tbody>
          </table>
        </section>

        <section className="shortcut-section">
          <h3>Native Tools & Files</h3>
          <table className="shortcut-table">
            <tbody>
              <tr>
                <td>Open in VS Code</td>
                <td>Header &amp; Diff toolbar button</td>
              </tr>
              <tr>
                <td>Open in Cursor</td>
                <td>Header &amp; Diff toolbar button</td>
              </tr>
              <tr>
                <td>Reveal in File Manager</td>
                <td>Header &amp; Diff toolbar button</td>
              </tr>
            </tbody>
          </table>
        </section>

        <div className="shortcut-modal-footer">
          <button
            type="button"
            className="primary"
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
