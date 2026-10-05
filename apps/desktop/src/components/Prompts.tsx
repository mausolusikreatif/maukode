import { needsContinue } from "../api/types";
import type { PermissionDecision, PermissionRequest } from "../api/types";
import { useApp } from "../state/store";
import type { ThreadState } from "../state/thread";

const CHOICES: { decision: PermissionDecision; label: string; testid: string; primary?: boolean }[] = [
  { decision: "allow_once", label: "Allow once", testid: "perm-allow-once", primary: true },
  { decision: "allow_session", label: "Allow for session", testid: "perm-allow-session" },
  { decision: "allow_private", label: "Run, hide result from AI", testid: "perm-allow-private" },
  { decision: "deny", label: "Deny", testid: "perm-deny" },
];

function PermissionCard({ request }: { request: PermissionRequest }) {
  const respond = useApp((s) => s.respond);
  return (
    <div data-testid="permission-card" className="permission">
      <div className="perm-head">
        <strong>{request.tool_name}</strong>
        <span className={`risk ${request.risk_level}`}>{request.risk_level}</span>
      </div>
      <div className="muted">{request.description}</div>
      {request.command && <pre>{request.command}</pre>}
      <div className="row">
        {CHOICES.map((c) => (
          <button
            key={c.decision}
            data-testid={c.testid}
            className={c.primary ? "primary" : ""}
            onClick={() => void respond(request.request_id, c.decision)}
          >
            {c.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Prompts({ thread }: { thread: ThreadState }) {
  const send = useApp((s) => s.send);
  const stopped = thread.stop;
  return (
    <div className="prompts">
      {thread.pending.map((p) => (
        <PermissionCard key={p.request_id} request={p} />
      ))}
      {thread.notices.map((n) => (
        <div key={n.id} data-testid="notice" className={`notice ${n.level}`}>
          {n.text}
        </div>
      ))}
      {!thread.running && stopped && needsContinue(stopped.reason) && (
        <div data-testid="stop-banner" className="notice warn row">
          <span>
            {stopped.reason === "max_steps" ? "Stopped at the step limit." : "Cut off by the output limit."} The task is
            probably unfinished.
          </span>
          <button data-testid="continue-button" className="primary" onClick={() => void send("continue")}>
            Continue
          </button>
        </div>
      )}
    </div>
  );
}
