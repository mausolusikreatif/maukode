---
name: safe-cargo-lock-version-bump
description: "Bump only project crates in Cargo.lock without touching third-party dependency versions that coincidentally share the same version number."
run_as_subagent: false
---
When bumping the project version in Cargo.lock files, do NOT use blanket `sed` replacement on the version string. Instead, parse the lock file and only replace the `version = "X"` line that immediately follows a `name = "maukode-*"` line. Use a script like: `python3 -c "import re; t=open('Cargo.lock').read(); t=re.sub(r'(name = \"maukode-[^\"]+\"\nversion = \")[^\"]+', r'\\g<1>NEW_VERSION', t); open('Cargo.lock','w').write(t)"`. Always verify with `grep -B1 'version = "
