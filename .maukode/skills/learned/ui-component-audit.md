---
name: ui-component-audit
description: "Audits a React codebase for raw HTML elements that should use shared design system components, producing a file-by-file findings report."
run_as_subagent: false
---
Scan the target codebase for raw `<button>`, `<input>`, and other HTML elements that bypass shared UI components. For each finding, report: file path, line number, the raw element, the recommended shared component replacement with appropriate variant/size, and severity (critical/medium/low). Also flag arbitrary Tailwind values like `text-[Npx]` that should use design tokens. Group findings by severity. Do NOT modify any files — produce only the audit report.
