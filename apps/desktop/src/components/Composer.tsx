import { useEffect, useRef, useState } from "react";
import { useApp } from "../state/store";

export function Composer({ running }: { running: boolean }) {
  const [text, setText] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const { send, stop } = useApp();

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const next = Math.min(el.scrollHeight, 220);
    el.style.height = `${Math.max(28, next)}px`;
  }, [text]);

  const submit = () => {
    const value = text.trim();
    if (!value || running) return;
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
    void send(value);
  };

  return (
    <div className={`composer ${running ? "running" : ""}`}>
      <textarea
        ref={textareaRef}
        data-testid="composer-input"
        value={text}
        placeholder={
          running
            ? "Agent is working…  Press Stop to interrupt"
            : "Message the agent…  Enter to send, Shift+Enter for a new line"
        }
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            submit();
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            submit();
          }
        }}
        rows={1}
      />
      {running ? (
        <button data-testid="stop-button" className="danger" onClick={() => void stop()} title="Interrupt the agent turn">
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
