import { useState } from "react";
import { Check, CircleDot, Circle, X } from "lucide-react";
import type { Plan, StepStatus } from "../api/types";
import { cn } from "@/lib/utils";

export interface PlanTimelineProps {
  plan: Plan;
  className?: string;
}

function getStatusIcon(status: StepStatus) {
  switch (status) {
    case "completed":
      return <Check className="size-3.5" />;
    case "in_progress":
      return <CircleDot className="size-3.5" />;
    case "pending":
      return <Circle className="size-3.5" />;
    case "failed":
      return <X className="size-3.5" />;
  }
}

function getStatusLabel(status: StepStatus): string {
  switch (status) {
    case "completed":
      return "Done";
    case "in_progress":
      return "Working";
    case "pending":
      return "Pending";
    case "failed":
      return "Failed";
  }
}

const statusColor: Record<StepStatus, string> = {
  completed: "text-success",
  in_progress: "text-primary",
  pending: "text-muted-foreground",
  failed: "text-destructive",
};

export function PlanTimeline({ plan, className = "" }: PlanTimelineProps) {
  const [collapsed, setCollapsed] = useState(false);
  const total = plan.steps.length;
  const completed = plan.steps.filter((s) => s.status === "completed").length;
  const inProgress = plan.steps.find((s) => s.status === "in_progress");

  return (
    <div
      className={cn("plan-timeline overflow-hidden rounded-lg border border-border bg-card/40", className)}
      data-testid="plan-timeline"
    >
      <div
        className="plan-header flex cursor-pointer items-center justify-between gap-2 px-3 py-2 hover:bg-accent/40"
        onClick={() => setCollapsed(!collapsed)}
      >
        <div className="plan-title-area flex items-center gap-2">
          <span className="plan-badge rounded-md bg-tool/15 px-2 py-0.5 text-[11px] font-medium text-tool">
            Plan
          </span>
          <span className="plan-title text-sm font-medium">{plan.title || "Execution Plan"}</span>
        </div>
        <div className="plan-meta flex items-center gap-2 text-xs text-muted-foreground">
          <span className="plan-progress font-mono" data-testid="plan-progress">
            {completed}/{total} completed
          </span>
          <button
            type="button"
            className="plan-toggle-btn text-primary hover:underline"
            onClick={(e) => {
              e.stopPropagation();
              setCollapsed(!collapsed);
            }}
            title={collapsed ? "Expand plan" : "Collapse plan"}
          >
            {collapsed ? "Expand" : "Collapse"}
          </button>
        </div>
      </div>

      {!collapsed && (
        <div className="plan-steps space-y-1 border-t border-border px-3 py-2">
          {plan.steps.map((step) => {
            const icon = getStatusIcon(step.status);
            const label = getStatusLabel(step.status);
            return (
              <div key={step.id} className={cn("plan-step flex items-center gap-2", step.status)} data-testid="plan-step">
                <span
                  className={cn("plan-step-icon", statusColor[step.status])}
                  data-testid="plan-step-icon"
                  title={label}
                >
                  {icon}
                </span>
                <span className="plan-step-title flex-1 text-sm" data-testid="plan-step-title">
                  {step.title}
                </span>
                {step.status === "in_progress" && (
                  <span
                    className="plan-step-tag working rounded-md bg-primary/15 px-2 py-0.5 text-[11px] text-primary"
                    data-testid="plan-step-status"
                  >
                    Working
                  </span>
                )}
                {step.status === "failed" && (
                  <span
                    className="plan-step-tag failed rounded-md bg-destructive/15 px-2 py-0.5 text-[11px] text-destructive"
                    data-testid="plan-step-status"
                  >
                    Failed
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}

      {collapsed && inProgress && (
        <div className="plan-collapsed-current flex items-center gap-2 border-t border-border px-3 py-2">
          <span className="plan-step-icon in_progress text-primary">{getStatusIcon("in_progress")}</span>
          <span className="plan-step-title current-title flex-1 text-sm">{inProgress.title}</span>
          <span className="plan-step-tag working rounded-md bg-primary/15 px-2 py-0.5 text-[11px] text-primary">
            Working
          </span>
        </div>
      )}
    </div>
  );
}
