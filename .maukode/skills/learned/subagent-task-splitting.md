---
name: subagent-task-splitting
description: "Split large analysis tasks into focused subagents by concern area to avoid context exhaustion"
run_as_subagent: false
---
When invoking a subagent for a large analysis task (e.g. auditing 10+ files across multiple dimensions), split it into 2-3 focused subagents instead. Each subagent should handle one concern area (e.g. "design system and tokens" vs "component-level UX issues") with a clear, bounded file list and specific output requirements. Run them in parallel and merge results.
