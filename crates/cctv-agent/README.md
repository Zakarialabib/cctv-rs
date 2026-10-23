# cctv-agent

The fleet reconciler for `cctv-rs`. It holds the desired state (from the
dashboard), diffs it against the observed state read through `cctv-core` and the
drivers, applies graded remediation, and records every change so a misconfigured
fleet can be rewound.

- `Reconciler` — `new` builds it around an audit trail; `reconcile` diffs a
  camera's desired and observed state and applies the remediation.
- `Audit` — `new` and `log`, one row per remediation (camera, change, grade).

See [`docs/architecture.md`](../../docs/architecture.md) for the full
architecture.
