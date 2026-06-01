# Security Model

## Overview

Cyrene implements defense-in-depth through multiple security layers:

1. **Autonomy Policy** — risk classification and default controls
2. **Sandboxing** — OS-level confinement to workspace boundary
3. **Shadow Execution** — dry-run before irreversible actions
4. **Approval Gates** — human-in-the-loop for high-stakes decisions
5. **Receipt Ledger** — immutable, signed audit trail
6. **Injection Scanner** — defense against prompt injection attacks

## Autonomy Levels

| Risk | Default | Behavior |
|------|---------|----------|
| Low | Auto | Executes automatically |
| Medium | Approval | Requires user approval |
| High | Blocked | Blocked until autonomy is explicitly raised |

## Sandboxing

All tool execution runs within an OS-level sandbox:
- **Linux**: Landlock LSM
- **macOS**: Seatbelt/sandbox-exec
- **Windows**: Job Objects/restricted tokens
- **Fallback**: Docker container

The sandbox confines file and process access to the configured workspace boundary.

## Approval Gates

Before any irreversible action, Cyrene:

1. Runs the plan in shadow mode (sandbox copy)
2. Produces a Projected Outcome Summary
3. Presents Approve / Rewrite / Abort options
4. Persists pending state across restarts
5. Times out and cancels on no response

## Receipt Ledger

Every action produces a signed, hash-chained receipt:

- **SHA-256** hash chain detects any tampering
- **Ed25519** signatures verify authenticity
- **Append-only** — no deletions or reordering
- **Verifiable** — `verify()` walks the chain and reports the first divergence

## Injection Scanner

All untrusted content (web pages, tool output, external messages) passes through:

- Heuristic pattern matching for known injection techniques
- Role-switch detection
- Exfiltration pattern detection
- Quarantine and logging on detection

## Best Practices

- Never raise autonomy without reviewing the implications
- Keep the command allowlist minimal
- Regularly run `cyrene doctor` to check security posture
- Review the receipt ledger periodically for unexpected actions
