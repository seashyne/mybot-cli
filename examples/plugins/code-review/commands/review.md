---
description: Review the current diff like a careful teammate
---
Review the current changes.

1. Run `git diff` (and `git status`) to see what changed.
2. Read the surrounding code for each hunk before judging it.
3. Look for: correctness bugs, unhandled errors, security issues
   (injection, path traversal, secrets), and missing tests.
4. Report findings as a list ordered by severity, each with `path:line`
   and a concrete suggestion. Say explicitly when you find nothing.

$ARGUMENTS
