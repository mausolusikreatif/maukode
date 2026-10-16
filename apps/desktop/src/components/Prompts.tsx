import { needsContinue } from "../api/types";
import type { PermissionDecision, PermissionRequest } from "../api/types";
import { useApp } from "../state/store";
import type { ThreadState } from "../state/thread";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";

const CHOICES: { decision: PermissionDecision; label: string; testid: string; primary?: boolean }[] = [
  { decision: "allow_once", label: "Allow once", testid: "perm-allow-once", primary: true },
  { decision: "allow_session", label: "Allow for session", testid: "perm-allow-session" },
  { decision: "allow_private", label: "Run, hide result from AI", testid: "perm-allow-private" },
  { decision: "deny", label: "Deny", testid: "perm-deny" },
];

const RISK_VARIANT: Record<string, "success" | "warning" | "destructive" | "default"> = {
  low: "success",
  medium: "warning",
  high: "destructive",
  critical: "destructive",
};

function PermissionCard({ request }: { request: PermissionRequest }) {
  const respond = useApp((s) => s.respond);
  return (
    <div
      data-testid="permission-card"
      className="permission flex flex-col rounded-lg border border-warning/40 bg-warning/5 p-3"
    >
      <div className="perm-head flex items-center justify-between gap-2">
        <strong className="font-mono text-sm text-tool">{request.tool_name}</strong>
        <Badge variant={RISK_VARIANT[request.risk_level] ?? "default"} className={cn("risk", request.risk_level)}>
          {request.risk_level}
        </Badge>
      </div>
      <div className="mt-1 text-sm text-muted-foreground">{request.description}</div>
      {request.command && (
        <pre
          className="scrollbar-thin mt-2 max-h-[30vh] overflow-x-auto overflow-y-auto rounded-md bg-background/70 p-2 font-mono text-xs"
        >
          {request.command}
        </pre>
      )}
      <div className="row mt-3 flex shrink-0 flex-wrap gap-2">
        {CHOICES.map((c) => (
          <Button
            key={c.decision}
            data-testid={c.testid}
            variant={c.primary ? "default" : "outline"}
            size="sm"
            onClick={() => void respond(request.request_id, c.decision)}
          >
            {c.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

const NOTICE_VARIANT: Record<string, "destructive" | "warning" | "default" | "info"> = {
  error: "destructive",
  warn: "warning",
  info: "info",
};

export function Prompts({ thread }: { thread: ThreadState }) {
  const send = useApp((s) => s.send);
  const stopped = thread.stop;
  return (
    <div className="prompts flex max-h-[50vh] shrink-0 flex-col gap-2 overflow-y-auto px-3 py-2 scrollbar-thin lg:max-h-none lg:overflow-visible lg:px-6">
      {thread.pending.map((p) => (
        <PermissionCard key={p.request_id} request={p} />
      ))}
      {thread.notices.map((n) => (
        <div
          key={n.id}
          data-testid="notice"
          className={cn(
            "notice rounded-md border px-3 py-2 text-sm",
            `notice-${n.level}`,
            n.level === "error" && "border-destructive/40 bg-destructive/10 text-destructive",
            n.level === "warn" && "border-warning/40 bg-warning/10 text-warning",
            !NOTICE_VARIANT[n.level] && "border-border bg-card/40 text-muted-foreground",
          )}
        >
          {n.text}
        </div>
      ))}
      {!thread.running && stopped && needsContinue(stopped.reason) && (
        <div
          data-testid="stop-banner"
          className="notice warn row flex items-center justify-between gap-3 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning"
        >
          <span>
            {stopped.reason === "max_steps" ? "Stopped at the step limit." : "Cut off by the output limit."} The task is
            probably unfinished.
          </span>
          <Button data-testid="continue-button" size="sm" onClick={() => void send("continue")}>
            Continue
          </Button>
        </div>
      )}
    </div>
  );
}
