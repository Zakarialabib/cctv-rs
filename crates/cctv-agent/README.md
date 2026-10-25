# cctv-agent

The fleet reconciler for `cctv-rs`. It holds the desired state (from the
dashboard), diffs it against the observed state read through `cctv-core` and the
drivers, applies **graded** remediation, and records every change so a
misconfigured fleet can be rewound.

The grade is the point. A camera that is merely unreachable earns a *retry*, one
that failed authentication earns a *reconfigure* (only an operator can fix it),
and one the fleet no longer has earns a *disable*. An error the reconciler does
not recognise is *notify* — never *disable*, because an unknown failure is not
evidence that the camera is gone.

- `Reconciler` — `new`, `set_desired`, `record_error`, `clear_error`,
  `reconcile`, `reconcile_all`.
- `Remediation` — one graded decision: the camera, the `Action`, the `Grade`, and
  a one-line reason.
- `Grade` — `None` < `Observe` < `Notify` < `Retry` < `Reconfigure` < `Disable`.
- `Action` — `Hold`, `Advance(CameraStateCurrent)`, `MarkOffline`,
  `TakeOutOfService`.
- `Audit` / `AuditEntry` — the append-only record: camera, change, grade,
  timestamp.

## Grading

| Observed | Grade | Action |
|---|---|---|
| matches its desired state | `None` | `Hold` |
| capability not present (`PtzDisabled`, `NotImplemented`) | `Observe` | `Hold` |
| confidence below the floor | `Observe` | `Hold` |
| refused, unclassified, unknown error | `Notify` | `Hold` |
| stepping toward the desired state | `Notify` | `Advance(..)` |
| offline / network / stream / capture failure | `Retry` | `MarkOffline` or `Hold` |
| authentication failed, operator disabled it | `Reconfigure` | `Hold` or `TakeOutOfService` |
| no camera matches the identity | `Disable` | `TakeOutOfService` |

The audit store is in memory here; the application persists it, because the
workspace owns no database.

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
