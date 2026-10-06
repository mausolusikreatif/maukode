import { useEffect, useRef, useState } from "react";
import { useApp } from "../state/store";

export function Composer({ running }: { running: boolean }) {
  const [text, setText] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const { send, stop, steer, queue } = useApp();

  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const next = Math.min(el.scrollHeight, 220);
    el.style.height = `${Math.max(28, next)}px`;
  }, [text]);

  const clearInput = () => {
    setText("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  };

  const handleSend = () => {
    const value = text.trim();
    if (!value || running) return;
    clearInput();
    void send(value);
  };

  const handleSteer = () => {
    const value = text.trim();
    if (!value) return;
    clearInput();
    void steer(value);
  };

  const handleQueue = () => {
    const value = text.trim();
    if (!value) return;
    clearInput();
    void queue(value);
  };

  const hasText = text.trim().length > 0;

  return (
    <div className={`composer ${running ? "running" : ""}`}>
      <textarea
        ref={textareaRef}
        data-testid="composer-input"
        value={text}
        placeholder={
          running
            ? "Agent is working…  Steer now or queue next"
            : "Message the agent…  Enter to send, Shift+Enter for a new line"
        }
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            if (running) {
              if (hasText) handleQueue();
            } else {
              handleSend();
            }
          } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
            e.preventDefault();
            if (running) {
              if (hasText) handleSteer();
            } else {
              handleSend();
            }
          }
        }}
        rows={1}
      />
      {running ? (
        <div className="composer-actions">
          {hasText && (
            <>
              <button
                type="button"
                data-testid="steer-button"
                className="primary steer-btn"
                onClick={handleSteer}
                title="Inject guidance at the next safe boundary (Cmd+Enter)"
              >
                Steer now
              </button>
              <button
                type="button"
                data-testid="queue-button"
                className="queue-btn"
                onClick={handleQueue}
                title="Queue follow-up for after the current turn (Enter)"
              >
                Queue next
              </button>
            </>
          )}
          <button
            type="button"
            data-testid="stop-button"
            className="danger"
            onClick={() => void stop()}
            title="Interrupt the agent turn"
          >
            Stop
          </button>
        </div>
      ) : (
        <button
          type="button"
          data-testid="send-button"
          className="primary"
          disabled={!hasText}
          onClick={handleSend}
        >
          Send
        </button>
      )}
    </div>
  );
}
