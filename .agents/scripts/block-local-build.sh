#!/usr/bin/env bash
set -euo pipefail

# Read hook input from stdin
input=$(cat)
cmd=$(echo "$input" | jq -r '.toolCall.args.CommandLine // empty' 2>/dev/null || true)

# Patterns that trigger heavy compilation, tests, or benchmarks on local machine
if [ -n "$cmd" ] && echo "$cmd" | grep -iE '\b(cargo\s+(build|test|clippy|check|run|bench|nextest|b|t|c)|(npm|pnpm|yarn|bun)\s+(test|run\s+(build|typecheck|test)|build)|(npx\s+)?playwright\s+test)\b' >/dev/null 2>&1; then
  jq -n --arg reason "Local tests and builds are disabled to prevent CPU burning when running agents in parallel. Please rely on GitHub CI for build and test verification." '{decision: "deny", reason: $reason}'
else
  jq -n '{decision: "allow"}'
fi
