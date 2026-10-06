import { useState } from "react";
import type { Plan, StepStatus } from "../api/types";

export interface PlanTimelineProps {
  plan: Plan;
  className?: string;
}

function getStatusIcon(status: StepStatus): string {
  switch (status) {
    case "completed":
      return "✓";
    case "in_progress":
      return "◉";
    case "pending":
      return "○";
    case "failed":
      return "✕";
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

export function PlanTimeline({ plan, className = "" }: PlanTimelineProps) {
  const [collapsed, setCollapsed] = useState(false);
  const total = plan.steps.length;
  const completed = plan.steps.filter((s) => s.status === "completed").length;
  const inProgress = plan.steps.find((s) => s.status === "in_progress");

  return (
    <div
      className={`plan-timeline ${className}`}
      data-testid="plan-timeline"
    >
      <div className="plan-header" onClick={() => setCollapsed(!collapsed)}>
        <div className="plan-title-area">
          <span className="plan-badge">Plan</span>
          <span className="plan-title">{plan.title || "Execution Plan"}</span>
        </div>
        <div className="plan-meta">
          <span className="plan-progress" data-testid="plan-progress">
            {completed}/{total} completed
          </span>
          <button
            type="button"
            className="link plan-toggle-btn"
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
        <div className="plan-steps">
          {plan.steps.map((step) => {
            const icon = getStatusIcon(step.status);
            const label = getStatusLabel(step.status);
            return (
              <div
                key={step.id}
                className={`plan-step ${step.status}`}
                data-testid="plan-step"
              >
                <span
                  className={`plan-step-icon ${step.status}`}
                  data-testid="plan-step-icon"
                  title={label}
                >
                  {icon}
                </span>
                <span className="plan-step-title" data-testid="plan-step-title">
                  {step.title}
                </span>
                {step.status === "in_progress" && (
                  <span
                    className="plan-step-tag working"
                    data-testid="plan-step-status"
                  >
                    Working
                  </span>
                )}
                {step.status === "failed" && (
                  <span
                    className="plan-step-tag failed"
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
        <div className="plan-collapsed-current">
          <span className="plan-step-icon in_progress">◉</span>
          <span className="plan-step-title current-title">
            {inProgress.title}
          </span>
          <span className="plan-step-tag working">Working</span>
        </div>
      )}
    </div>
  );
}
