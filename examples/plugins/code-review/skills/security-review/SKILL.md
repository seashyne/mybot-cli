---
name: security-review
description: Audit a change for security problems before it ships
---
# Security review

Work through these checks against the diff and the files it touches.

## Input handling
- Is any user-controlled string passed to a shell, SQL query, or HTML output?
- Are paths built from user input without canonicalisation or a root check?

## Secrets
- Any hardcoded tokens, keys, passwords, or connection strings?
- Are secrets logged, echoed in errors, or written to session files?

## Network
- Are URLs taken from user input fetched without validation (SSRF)?
- Is TLS verification disabled anywhere?

## Filesystem and permissions
- Does a write escape the project directory?
- Is a file created with overly broad permissions?

## Output
Report only issues you can point at with `path:line`. If a check does not
apply, say so in one line instead of inventing a finding.
