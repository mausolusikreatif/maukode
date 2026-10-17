import { useEffect, useRef, useState } from "react";
import { useApp } from "../state/store";
import type { ThreadState } from "../state/thread";

export function Composer({ thread }: { thread: ThreadState }) {
  const { send, queue, steer, stop, clearQueue } = useApp.getState();
  const [text, setText] = useState("");
  const ref = useRef<HTMLTextAreaElement>(null);
  const running = thread.running;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [text]);

  const submit = () => {
    const t = text.trim();
    if (!t) return;
    setText("");
    if (running) void queue(t);
    else void send(t);
  };

  return (
    <div className="composer">
      {thread.queue.length > 0 && (
        <div className="queue">
          <span>{thread.queue.length} pesan antre, dikirim setelah giliran ini</span>
          <button className="btn quiet small" onClick={() => void clearQueue()}>Kosongkan</button>
        </div>
      )}
      <textarea
        ref={ref} rows={1} value={text} aria-label="Pesan untuk agent"
        placeholder={running ? "Agent sedang bekerja. Pesan baru masuk antrean." : "Tulis tugas untuk agent"}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          const touch = window.matchMedia("(pointer: coarse)").matches;
          if (e.key === "Enter" && !e.shiftKey && !touch) {
            e.preventDefault();
            submit();
          }
        }}
      />
      <div className="composer-row">
        <span className="small muted">{running ? "berjalan" : "siap"}</span>
        <span style={{ display: "flex", gap: 6 }}>
          {running && (
            <>
              <button className="btn" disabled={!text.trim()} onClick={() => { const t = text.trim(); setText(""); void steer(t); }}>
                Arahkan
              </button>
              <button className="btn deny" onClick={() => void stop()}>Hentikan</button>
            </>
          )}
          <button className="btn send" disabled={!text.trim()} onClick={submit}>
            {running ? "Antre" : "Kirim"}
          </button>
        </span>
      </div>
    </div>
  );
}
