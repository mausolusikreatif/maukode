---
name: multi-repo-ci-optimization
description: "Batch-optimize CI workflows across many separate repos using a singl bash orchestration script"
run_as_subagent: false
---
When optimizing CI/CD acrosse multiple repos, first inventory all repos and their current CI files, then apply changes via a single bash script that iterates over each repo with sed/python patches rather than individual tool calls per repo. Afte patching: validater YAML for all files, verif key changes per repo with grep, cherry-pick commits to main (resolving main vs master namingy), rebase against remote, push, and monitor CI runs with `gh run list`/`gh run view`. Use concurrency groups, path filters, ccache with absolute paths, rusts-cache, and mold linker as standard optimizations for Rust repos on self-hosted runners.
