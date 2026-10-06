import { useState } from "react";
import { useApp } from "../state/store";

export function Composer({ running }: { running: boolean }) {
  const [text, setText] = useState("");
  const { send, stop } = useApp();

  const submit = () => {
    const value = text.trim();
    if (!value || running) return;
    setText("");
    void send(value);
  };

  return (
    <div className="composer">
      <textarea
        data-testid="composer-input"
        value={text}
        placeholder="Message the agent…  Enter to send, Shift+Enter for a new line"
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            submit();
          }
        }}
        rows={1}
      />
      {running ? (
        <button data-testid="stop-button" className="danger" onClick={() => void stop()}>
          Stop
        </button>
      ) : (
        <button data-testid="send-button" className="primary" disabled={!text.trim()} onClick={submit}>
          Send
        </button>
      )}
    </div>
  );
}
