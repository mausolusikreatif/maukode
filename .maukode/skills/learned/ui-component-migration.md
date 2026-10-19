---
name: ui-component-migration
description: "Systematically replace raw HTML elements with shared design system components across a codebase"
run_as_subagent: false
---
When migrating raw `<button>` or other HTML elements to a shared component: 1) Read the target component definition first to catalog all valid variants and sizes. 2) grep the entire codebase for raw elements to scope the work. 3) Convert file-by-file, reading exact content before each edit to avoid whitespace mismatches. 4) Distinguish interactive list items (with data-index, keyboard nav) from toolbar buttons — list items should stay raw. 5) After all edits, grep for remaining raw elements and any invalid variant names. 6) Verify all icon imports exist in each modified file.
