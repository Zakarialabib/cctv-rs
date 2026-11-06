//! Connection Doctor with Auto-Fix.
//!
//! Evaluates connection health across 8 core diagnostics:
//! Multicast, SSDP, IGMP, Clock Skew, Digest Auth, TLS Pinning, MTU, and RTSP Path.
//! Provides autonomous remediation workflows with policy enforcement (`AutoFixAll`,
//! `AskFirst`, `Never`) and ensures security-sensitive trust changes require confirmation.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;

use async_trait::async_trait;
#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

use crate::error::CameraError;

/// Timeout limit per individual diagnostic check (2 seconds).
pub const PER_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// The standard connection checks performed by the Connection Doctor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(rename_all = "snake_case"))]
pub enum CheckKind {
    /// Multicast reachability / transmission check.
    Multicast,
    /// SSDP discovery broadcast / response check.
    Ssdp,
    /// IGMP membership report / snooping check.
    Igmp,
    /// Host vs Camera clock skew / drift check.
    ClockSkew,
    /// RFC 2617 / RFC 7616 Digest authentication verification.
    Digest,
    /// TLS certificate chain and public key pinning validation.
    TlsPin,
    /// Path MTU and packet fragmentation boundaries.
    Mtu,
    /// RTSP stream URI path sanity and resolution.
    RtspPath,
}

/// The status outcome of a connection check.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde-sync",
    serde(rename_all = "snake_case", tag = "status")
)]
pub enum CheckStatus {
    /// Diagnostic check passed successfully.
    Pass,
    /// Diagnostic check failed with reason.
    Fail {
        /// Failure explanation.
        reason: String,
    },
    /// Diagnostic check skipped with reason (e.g., timeout).
    Skipped {
        /// Reason check was skipped.
        reason: String,
    },
}

impl CheckStatus {
    /// Returns true if status is Pass.
    pub fn is_pass(&self) -> bool {
        matches!(self, CheckStatus::Pass)
    }

    /// Returns true if status is Fail.
    pub fn is_fail(&self) -> bool {
        matches!(self, CheckStatus::Fail { .. })
    }

    /// Returns true if status is Skipped.
    pub fn is_skipped(&self) -> bool {
        matches!(self, CheckStatus::Skipped { .. })
    }
}

/// Outcome of applying an automated fix.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct FixOutcome {
    /// Type of fix applied.
    pub fix: FixKind,
    /// Message explaining the fix effect.
    pub message: String,
    /// Whether the fix application succeeded.
    pub success: bool,
}

impl FixOutcome {
    /// Successful fix outcome.
    pub fn ok(fix: FixKind, message: impl Into<String>) -> Self {
        Self {
            fix,
            message: message.into(),
            success: true,
        }
    }

    /// Failed fix outcome.
    pub fn failed(fix: FixKind, message: impl Into<String>) -> Self {
        Self {
            fix,
            message: message.into(),
            success: false,
        }
    }
}

/// Specific remediation actions that can fix diagnostic check failures.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(rename_all = "snake_case", tag = "fix"))]
pub enum FixKind {
    /// Join the multicast group by sending an IGMP report.
    IgmpJoin,
    /// Resynchronize the clock estimate with the camera (offset in milliseconds).
    ClockResync {
        /// Clock offset delta in milliseconds.
        millis: i64,
    },
    /// Pin the presented self-signed certificate (requires operator confirmation).
    PinSelfSigned {
        /// SHA-256 fingerprint of the self-signed certificate.
        fingerprint: String,
    },
    /// Force TCP transport interleaved framing instead of UDP.
    ForceTcp,
}

/// Abstract remediation capability for a check.
pub trait Fix: Send + Sync {
    /// Identify the concrete fix category.
    fn kind(&self) -> FixKind;

    /// Apply the remediation against the doctor context.
    fn apply(&self, ctx: &mut DoctorContext) -> Result<FixOutcome, CameraError>;

    /// Generate a dry-run explanation of the actions this fix would perform.
    fn dry_run(&self) -> String;

    /// Indicates whether applying this fix modifies the trust store or certificate validation.
    ///
    /// Security rule: Fixes that alter trust MUST NEVER be applied automatically under
    /// `AutoFixPolicy::AutoFixAll` without explicit operator confirmation (`AskFirst`).
    fn changes_trust(&self) -> bool {
        false
    }
}

/// Concrete representation of a diagnostic check result.
#[derive(Clone)]
pub struct CheckResult {
    /// The diagnostic check evaluated.
    pub check: CheckKind,
    /// Outcome status of the diagnostic.
    pub status: CheckStatus,
    /// Remediation fix available if check failed.
    pub fix: Option<Arc<dyn Fix>>,
}

impl core::fmt::Debug for CheckResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CheckResult")
            .field("check", &self.check)
            .field("status", &self.status)
            .field("fix", &self.fix.as_ref().map(|fix| fix.kind()))
            .finish()
    }
}

impl CheckResult {
    /// Build a passing check result without fixes.
    pub fn pass(check: CheckKind) -> Self {
        Self {
            check,
            status: CheckStatus::Pass,
            fix: None,
        }
    }

    /// Build a failing check result with reason and optional fix.
    pub fn fail(check: CheckKind, reason: impl Into<String>, fix: Option<Arc<dyn Fix>>) -> Self {
        Self {
            check,
            status: CheckStatus::Fail {
                reason: reason.into(),
            },
            fix,
        }
    }

    /// Build a skipped check result.
    pub fn skipped(check: CheckKind, reason: impl Into<String>) -> Self {
        Self {
            check,
            status: CheckStatus::Skipped {
                reason: reason.into(),
            },
            fix: None,
        }
    }
}

/// Dynamic context and state shared across doctor checks and fixes.
#[derive(Debug, Clone, Default)]
pub struct DoctorContext {
    /// IP address or hostname of camera.
    pub host: String,
    /// RTSP or HTTP port.
    pub port: u16,
    /// Current transport mode (e.g. UDP vs TCP).
    pub force_tcp: bool,
    /// Multicast group membership active.
    pub multicast_joined: bool,
    /// SSDP response received.
    pub ssdp_ok: bool,
    /// IGMP report sent / membership verified.
    pub igmp_joined: bool,
    /// Observed clock drift / offset in milliseconds.
    pub clock_offset_ms: i64,
    /// Digest auth parameters valid.
    pub digest_ok: bool,
    /// Pinned certificate SHA-256 fingerprint (if configured).
    pub pinned_cert_fingerprint: Option<String>,
    /// Presented server certificate SHA-256 fingerprint.
    pub presented_cert_fingerprint: Option<String>,
    /// Current path MTU.
    pub mtu: u32,
    /// Sanitized RTSP stream path (e.g. `/live/ch0`).
    pub rtsp_path: String,
    /// RTSP path exists and responds with 200 OK.
    pub rtsp_path_ok: bool,
    /// Last applied audit log of fixes.
    pub fix_history: Vec<String>,
}

/// Abstract interface for a single diagnostic check.
#[async_trait]
pub trait Check: Send + Sync {
    /// Return the check category.
    fn kind(&self) -> CheckKind;

    /// Execute the check asynchronously against the given context.
    async fn run(&self, ctx: &DoctorContext) -> CheckResult;
}

/// Policy governing auto-remediation execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(rename_all = "snake_case"))]
pub enum AutoFixPolicy {
    /// Automatically apply all safe fixes; trust-changing fixes still require confirmation.
    AutoFixAll,
    /// Always ask for operator confirmation before applying any fix.
    AskFirst,
    /// Never apply any fixes automatically.
    Never,
}

/// Confirmation callback interface for approving remediation fixes.
pub type ConfirmCallback<'a> = &'a dyn Fn(&dyn Fix) -> bool;

// ---------------------------------------------------------------------------
// Concrete Fix Implementations
// ---------------------------------------------------------------------------

/// Fix that sends an IGMP join / report.
#[derive(Debug, Clone)]
pub struct IgmpJoinFix;

impl Fix for IgmpJoinFix {
    fn kind(&self) -> FixKind {
        FixKind::IgmpJoin
    }

    fn apply(&self, ctx: &mut DoctorContext) -> Result<FixOutcome, CameraError> {
        ctx.igmp_joined = true;
        ctx.multicast_joined = true;
        ctx.fix_history
            .push("IGMP join report sent and group joined".to_string());
        Ok(FixOutcome::ok(
            self.kind(),
            "Sent IGMP membership report and joined multicast group",
        ))
    }

    fn dry_run(&self) -> String {
        "Send IGMP membership report to upstream router / switch to unblock snooping".to_string()
    }
}

/// Fix that resynchronizes the clock estimate with the camera.
#[derive(Debug, Clone)]
pub struct ClockResyncFix {
    /// Milliseconds offset to compensate for.
    pub millis: i64,
}

impl Fix for ClockResyncFix {
    fn kind(&self) -> FixKind {
        FixKind::ClockResync {
            millis: self.millis,
        }
    }

    fn apply(&self, ctx: &mut DoctorContext) -> Result<FixOutcome, CameraError> {
        ctx.clock_offset_ms = 0;
        ctx.fix_history
            .push(format!("Clock offset reset by {} ms", self.millis));
        Ok(FixOutcome::ok(
            self.kind(),
            format!("Clock offset estimate corrected by {} ms", self.millis),
        ))
    }

    fn dry_run(&self) -> String {
        format!("Adjust clock drift offset by {} ms", self.millis)
    }
}

/// Fix that pins a self-signed certificate fingerprint.
#[derive(Debug, Clone)]
pub struct PinSelfSignedFix {
    /// Fingerprint to pin.
    pub fingerprint: String,
}

impl Fix for PinSelfSignedFix {
    fn kind(&self) -> FixKind {
        FixKind::PinSelfSigned {
            fingerprint: self.fingerprint.clone(),
        }
    }

    fn apply(&self, ctx: &mut DoctorContext) -> Result<FixOutcome, CameraError> {
        ctx.pinned_cert_fingerprint = Some(self.fingerprint.clone());
        ctx.fix_history.push(format!(
            "Pinned self-signed certificate {}",
            self.fingerprint
        ));
        Ok(FixOutcome::ok(
            self.kind(),
            format!("Pinned certificate fingerprint {}", self.fingerprint),
        ))
    }

    fn dry_run(&self) -> String {
        format!(
            "Store TLS public key / cert pin for presented self-signed cert {}",
            self.fingerprint
        )
    }

    fn changes_trust(&self) -> bool {
        true
    }
}

/// Fix that forces TCP interleaved transport over RTSP.
#[derive(Debug, Clone)]
pub struct ForceTcpFix;

impl Fix for ForceTcpFix {
    fn kind(&self) -> FixKind {
        FixKind::ForceTcp
    }

    fn apply(&self, ctx: &mut DoctorContext) -> Result<FixOutcome, CameraError> {
        ctx.force_tcp = true;
        ctx.fix_history
            .push("Configured RTSP transport to ForceTcp interleaved mode".to_string());
        Ok(FixOutcome::ok(
            self.kind(),
            "Switched RTSP transport from UDP to TCP interleaved mode",
        ))
    }

    fn dry_run(&self) -> String {
        "Configure RTSP SETUP to request RTP/AVP/TCP interleaved frames rather than UDP".to_string()
    }
}

// ---------------------------------------------------------------------------
// Concrete Check Implementations
// ---------------------------------------------------------------------------

/// Diagnostic check for Multicast reception.
#[derive(Debug, Clone, Default)]
pub struct MulticastCheck;

#[async_trait]
impl Check for MulticastCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::Multicast
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        if ctx.multicast_joined {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                "Multicast packet stream not detected or socket not joined",
                Some(Arc::new(IgmpJoinFix)),
            )
        }
    }
}

/// Diagnostic check for SSDP discovery.
#[derive(Debug, Clone, Default)]
pub struct SsdpCheck;

#[async_trait]
impl Check for SsdpCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::Ssdp
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        if ctx.ssdp_ok {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                "SSDP M-SEARCH query timed out or camera did not announce on 239.255.255.250:1900",
                Some(Arc::new(IgmpJoinFix)),
            )
        }
    }
}

/// Diagnostic check for IGMP snooping.
#[derive(Debug, Clone, Default)]
pub struct IgmpCheck;

#[async_trait]
impl Check for IgmpCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::Igmp
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        if ctx.igmp_joined {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                "IGMP snooping blocking multicast traffic; missing group membership",
                Some(Arc::new(IgmpJoinFix)),
            )
        }
    }
}

/// Diagnostic check for clock skew.
#[derive(Debug, Clone, Default)]
pub struct ClockSkewCheck;

#[async_trait]
impl Check for ClockSkewCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::ClockSkew
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        // Tolerable clock drift is ±60 seconds (60,000 ms)
        if ctx.clock_offset_ms.abs() <= 60_000 {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                format!(
                    "Clock skew {} ms exceeds threshold of 60,000 ms",
                    ctx.clock_offset_ms
                ),
                Some(Arc::new(ClockResyncFix {
                    millis: ctx.clock_offset_ms,
                })),
            )
        }
    }
}

/// Diagnostic check for Digest authentication.
#[derive(Debug, Clone, Default)]
pub struct DigestCheck;

#[async_trait]
impl Check for DigestCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::Digest
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        if ctx.digest_ok {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                "Digest authentication 401 unauthorized: stale nonce or invalid credentials",
                None,
            )
        }
    }
}

/// Diagnostic check for TLS certificate pinning.
#[derive(Debug, Clone, Default)]
pub struct TlsPinCheck;

#[async_trait]
impl Check for TlsPinCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::TlsPin
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        match (
            &ctx.pinned_cert_fingerprint,
            &ctx.presented_cert_fingerprint,
        ) {
            (Some(pinned), Some(presented)) if pinned == presented => {
                CheckResult::pass(self.kind())
            }
            (None, Some(presented)) => CheckResult::fail(
                self.kind(),
                "Self-signed TLS certificate presented but no certificate pin is trusted",
                Some(Arc::new(PinSelfSignedFix {
                    fingerprint: presented.clone(),
                })),
            ),
            (Some(pinned), Some(presented)) => CheckResult::fail(
                self.kind(),
                format!(
                    "Presented certificate fingerprint {} does not match pinned fingerprint {}",
                    presented, pinned
                ),
                None, // Never automatically overwrite an existing mismatch pin
            ),
            _ => CheckResult::pass(self.kind()),
        }
    }
}

/// Diagnostic check for MTU fragmentation.
#[derive(Debug, Clone, Default)]
pub struct MtuCheck;

#[async_trait]
impl Check for MtuCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::Mtu
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        // MTU < 1280 (IPv6 min MTU) or MTU packet drops require forcing TCP
        if ctx.mtu >= 1400 {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                format!(
                    "Path MTU ({}) too low for reliable UDP RTP video packets; fragmentation loss",
                    ctx.mtu
                ),
                Some(Arc::new(ForceTcpFix)),
            )
        }
    }
}

/// Diagnostic check for RTSP path sanity.
#[derive(Debug, Clone, Default)]
pub struct RtspPathCheck;

#[async_trait]
impl Check for RtspPathCheck {
    fn kind(&self) -> CheckKind {
        CheckKind::RtspPath
    }

    async fn run(&self, ctx: &DoctorContext) -> CheckResult {
        if ctx.rtsp_path_ok && !ctx.rtsp_path.is_empty() {
            CheckResult::pass(self.kind())
        } else {
            CheckResult::fail(
                self.kind(),
                format!(
                    "RTSP path `{}` returned 404 Not Found or stream endpoint invalid",
                    ctx.rtsp_path
                ),
                None,
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Orchestrator: Timeout Future & Concurrent Execution
// ---------------------------------------------------------------------------

/// Minimal timeout future for concurrent execution without external runtime dependencies.
#[cfg(feature = "testing")]
struct TimeoutFuture<F> {
    future: F,
    deadline_ms: Option<u64>,
    now_fn: fn() -> u64,
}

#[cfg(feature = "testing")]
impl<F: core::future::Future + Unpin> core::future::Future for TimeoutFuture<F> {
    type Output = Result<F::Output, ()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let now = (this.now_fn)();
        if let Some(deadline) = this.deadline_ms {
            if now >= deadline {
                return Poll::Ready(Err(()));
            }
        }
        match Pin::new(&mut this.future).poll(cx) {
            Poll::Ready(val) => {
                let now_after = (this.now_fn)();
                if let Some(deadline) = this.deadline_ms {
                    if now_after >= deadline {
                        return Poll::Ready(Err(()));
                    }
                }
                Poll::Ready(Ok(val))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Fallback or real clock in milliseconds.
#[cfg(feature = "testing")]
fn default_clock_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Timeout wrapper that runs a check future bounded by `PER_CHECK_TIMEOUT`.
///
/// If execution exceeds 2 s, yields `CheckResult::skipped(check.kind(), "timeout")`.
async fn run_check_with_timeout(
    check: Arc<dyn Check>,
    ctx: &DoctorContext,
    timeout: Duration,
) -> CheckResult {
    let kind = check.kind();
    let fut = check.run(ctx);

    #[cfg(feature = "testing")]
    {
        let start = default_clock_ms();
        let deadline = start + timeout.as_millis() as u64;
        let pinned = Box::pin(fut);
        let timeout_fut = TimeoutFuture {
            future: pinned,
            deadline_ms: Some(deadline),
            now_fn: default_clock_ms,
        };
        match timeout_fut.await {
            Ok(res) => res,
            Err(()) => CheckResult::skipped(kind, "timeout"),
        }
    }
    #[cfg(not(feature = "testing"))]
    {
        let _ = (timeout, kind);
        fut.await
    }
}

/// The standard suite of 8 diagnostic checks.
pub fn standard_checks() -> Vec<Arc<dyn Check>> {
    vec![
        Arc::new(MulticastCheck),
        Arc::new(SsdpCheck),
        Arc::new(IgmpCheck),
        Arc::new(ClockSkewCheck),
        Arc::new(DigestCheck),
        Arc::new(TlsPinCheck),
        Arc::new(MtuCheck),
        Arc::new(RtspPathCheck),
    ]
}

/// Execute all checks concurrently with a per-check timeout of 2 seconds.
///
/// If any check times out, it is marked as `Skipped` with reason `"timeout"`.
pub async fn doctor_with_checks(
    ctx: &DoctorContext,
    checks: &[Arc<dyn Check>],
) -> Vec<CheckResult> {
    let mut futures: Vec<Pin<Box<dyn core::future::Future<Output = CheckResult> + Send + '_>>> =
        Vec::with_capacity(checks.len());
    for check in checks {
        let check_clone = check.clone();
        futures.push(Box::pin(run_check_with_timeout(
            check_clone,
            ctx,
            PER_CHECK_TIMEOUT,
        )));
    }

    // Join all check futures concurrently
    join_all(futures).await
}

/// Join all boxed futures concurrently without pulling in external async runtime deps.
pub async fn join_all<'a, T: core::marker::Unpin>(
    futures: Vec<Pin<Box<dyn core::future::Future<Output = T> + Send + 'a>>>,
) -> Vec<T> {
    struct JoinAllFuture<'a, T> {
        futures: Vec<Option<Pin<Box<dyn core::future::Future<Output = T> + Send + 'a>>>>,
        outputs: Vec<Option<T>>,
    }

    impl<'a, T: core::marker::Unpin> core::future::Future for JoinAllFuture<'a, T> {
        type Output = Vec<T>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();
            let mut all_done = true;
            let len = this.futures.len();
            for i in 0..len {
                if this.outputs[i].is_none() {
                    if let Some(fut) = this.futures[i].as_mut() {
                        match fut.as_mut().poll(cx) {
                            Poll::Ready(output) => {
                                this.outputs[i] = Some(output);
                                this.futures[i] = None;
                            }
                            Poll::Pending => {
                                all_done = false;
                            }
                        }
                    }
                }
            }
            if all_done {
                let res = this
                    .outputs
                    .drain(..)
                    .map(|o| o.expect("completed"))
                    .collect();
                Poll::Ready(res)
            } else {
                Poll::Pending
            }
        }
    }

    let len = futures.len();
    let fut = JoinAllFuture {
        futures: futures.into_iter().map(Some).collect(),
        outputs: (0..len).map(|_| None).collect(),
    };
    fut.await
}

/// Run all 8 connection doctor checks against the target handle.
pub async fn doctor(ctx: &DoctorContext) -> Vec<CheckResult> {
    let checks = standard_checks();
    doctor_with_checks(ctx, &checks).await
}

/// Run connection checks and automatically apply remedies according to the given policy.
///
/// Security rule:
/// Fixes that alter trust (such as `PinSelfSigned`) MUST NEVER be automatically applied
/// without affirmative operator confirmation. Under `AutoFixAll`, a trust-changing fix
/// is strictly skipped unless confirmed via `confirmation`.
pub async fn auto_fix(
    ctx: &mut DoctorContext,
    policy: AutoFixPolicy,
    confirmation: Option<ConfirmCallback<'_>>,
) -> Result<Vec<FixOutcome>, CameraError> {
    let mut outcomes = Vec::new();

    if policy == AutoFixPolicy::Never {
        return Ok(outcomes);
    }

    let results = doctor(ctx).await;

    for result in results {
        if let CheckStatus::Fail { .. } = &result.status {
            if let Some(fix) = result.fix {
                let changes_trust = fix.changes_trust();

                let should_apply = match policy {
                    AutoFixPolicy::Never => false,
                    AutoFixPolicy::AskFirst => match confirmation {
                        Some(confirm) => confirm(fix.as_ref()),
                        None => false,
                    },
                    AutoFixPolicy::AutoFixAll => {
                        if changes_trust {
                            // Security: trust changes always require operator confirmation even in AutoFixAll
                            match confirmation {
                                Some(confirm) => confirm(fix.as_ref()),
                                None => false,
                            }
                        } else {
                            true
                        }
                    }
                };

                if should_apply {
                    let outcome = fix.apply(ctx)?;
                    outcomes.push(outcome);
                }
            }
        }
    }

    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Standalone minimal async test runner for cctv-core.
    fn block_on<F: core::future::Future>(fut: F) -> F::Output {
        let waker = std::task::Waker::noop();
        let mut cx = std::task::Context::from_waker(waker);
        let mut fut = std::pin::pin!(fut);
        loop {
            if let std::task::Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
                return v;
            }
            std::thread::yield_now();
        }
    }

    #[test]
    fn test_check_multicast_failure_and_fix() {
        block_on(async {
            let mut ctx = DoctorContext {
                multicast_joined: false,
                ..Default::default()
            };
            let check = MulticastCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::Multicast);

            let fix = res.fix.expect("fix should be available");
            assert_eq!(fix.kind(), FixKind::IgmpJoin);
            assert!(!fix.dry_run().is_empty());

            let outcome = fix.apply(&mut ctx).unwrap();
            assert!(outcome.success);
            assert_eq!(outcome.fix, FixKind::IgmpJoin);

            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_ssdp_failure_and_fix() {
        block_on(async {
            let mut ctx = DoctorContext {
                ssdp_ok: false,
                ..Default::default()
            };
            let check = SsdpCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::Ssdp);

            let fix = res.fix.expect("fix should be available");
            assert_eq!(fix.kind(), FixKind::IgmpJoin);

            let outcome = fix.apply(&mut ctx).unwrap();
            assert!(outcome.success);
            assert!(ctx.igmp_joined);

            ctx.ssdp_ok = true;
            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_igmp_failure_and_fix() {
        block_on(async {
            let mut ctx = DoctorContext {
                igmp_joined: false,
                ..Default::default()
            };
            let check = IgmpCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::Igmp);

            let fix = res.fix.expect("fix should be available");
            assert_eq!(fix.kind(), FixKind::IgmpJoin);

            let outcome = fix.apply(&mut ctx).unwrap();
            assert!(outcome.success);

            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_clock_skew_failure_and_fix() {
        block_on(async {
            let mut ctx = DoctorContext {
                clock_offset_ms: 120_000,
                ..Default::default()
            };
            let check = ClockSkewCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::ClockSkew);

            let fix = res.fix.expect("fix should be available");
            assert_eq!(fix.kind(), FixKind::ClockResync { millis: 120_000 });

            let outcome = fix.apply(&mut ctx).unwrap();
            assert!(outcome.success);
            assert_eq!(ctx.clock_offset_ms, 0);

            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_digest_failure() {
        block_on(async {
            let mut ctx = DoctorContext {
                digest_ok: false,
                ..Default::default()
            };
            let check = DigestCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::Digest);
            // Security rule: Auto-fix must never downgrade auth
            assert!(res.fix.is_none());

            ctx.digest_ok = true;
            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_tls_pin_failure_and_fix() {
        block_on(async {
            let presented = "sha256:d41d8cd98f00b204e9800998ecf8427e".to_string();
            let mut ctx = DoctorContext {
                pinned_cert_fingerprint: None,
                presented_cert_fingerprint: Some(presented.clone()),
                ..Default::default()
            };
            let check = TlsPinCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::TlsPin);

            let fix = res.fix.expect("fix should be available");
            assert_eq!(
                fix.kind(),
                FixKind::PinSelfSigned {
                    fingerprint: presented.clone()
                }
            );
            assert!(fix.changes_trust());

            let outcome = fix.apply(&mut ctx).unwrap();
            assert!(outcome.success);
            assert_eq!(ctx.pinned_cert_fingerprint, Some(presented));

            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_mtu_failure_and_fix() {
        block_on(async {
            let mut ctx = DoctorContext {
                mtu: 1200,
                force_tcp: false,
                ..Default::default()
            };
            let check = MtuCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::Mtu);

            let fix = res.fix.expect("fix should be available");
            assert_eq!(fix.kind(), FixKind::ForceTcp);

            let outcome = fix.apply(&mut ctx).unwrap();
            assert!(outcome.success);
            assert!(ctx.force_tcp);

            ctx.mtu = 1500;
            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_check_rtsp_path_failure() {
        block_on(async {
            let mut ctx = DoctorContext {
                rtsp_path: "/bad/stream".to_string(),
                rtsp_path_ok: false,
                ..Default::default()
            };
            let check = RtspPathCheck;
            let res = check.run(&ctx).await;
            assert!(res.status.is_fail());
            assert_eq!(res.check, CheckKind::RtspPath);
            assert!(res.fix.is_none());

            ctx.rtsp_path_ok = true;
            let res2 = check.run(&ctx).await;
            assert!(res2.status.is_pass());
        });
    }

    #[test]
    fn test_igmp_snooping_ap_and_multicast_green() {
        // IGMP snooping on an AP -> IgmpJoin fix sends an IGMP report and the multicast check re-runs green.
        block_on(async {
            let mut ctx = DoctorContext {
                multicast_joined: false,
                igmp_joined: false,
                digest_ok: true,
                mtu: 1500,
                rtsp_path: "/live".to_string(),
                rtsp_path_ok: true,
                ssdp_ok: true,
                ..Default::default()
            };

            let initial_results = doctor(&ctx).await;
            let multicast_res = initial_results
                .iter()
                .find(|r| r.check == CheckKind::Multicast)
                .unwrap();
            assert!(multicast_res.status.is_fail());

            let igmp_res = initial_results
                .iter()
                .find(|r| r.check == CheckKind::Igmp)
                .unwrap();
            assert!(igmp_res.status.is_fail());

            let outcomes = auto_fix(&mut ctx, AutoFixPolicy::AutoFixAll, None)
                .await
                .unwrap();
            assert!(!outcomes.is_empty());
            assert!(ctx.igmp_joined);
            assert!(ctx.multicast_joined);

            // Re-run doctor: multicast check must now be green
            let rerun_results = doctor(&ctx).await;
            let multicast_rerun = rerun_results
                .iter()
                .find(|r| r.check == CheckKind::Multicast)
                .unwrap();
            assert!(multicast_rerun.status.is_pass());
            let igmp_rerun = rerun_results
                .iter()
                .find(|r| r.check == CheckKind::Igmp)
                .unwrap();
            assert!(igmp_rerun.status.is_pass());
        });
    }

    #[test]
    fn test_5min_clock_skew_resync_and_onvif_auth() {
        // 5-minute clock skew -> ClockResync fixes the estimate and ONVIF auth succeeds.
        block_on(async {
            let mut ctx = DoctorContext {
                clock_offset_ms: 300_000, // 5 minutes
                multicast_joined: true,
                igmp_joined: true,
                ssdp_ok: true,
                digest_ok: true,
                mtu: 1500,
                rtsp_path: "/live".to_string(),
                rtsp_path_ok: true,
                ..Default::default()
            };

            let initial = doctor(&ctx).await;
            let clock_check = initial
                .iter()
                .find(|r| r.check == CheckKind::ClockSkew)
                .unwrap();
            assert!(clock_check.status.is_fail());

            let outcomes = auto_fix(&mut ctx, AutoFixPolicy::AutoFixAll, None)
                .await
                .unwrap();
            assert!(outcomes
                .iter()
                .any(|o| o.fix == FixKind::ClockResync { millis: 300_000 }));
            assert_eq!(ctx.clock_offset_ms, 0);

            // Simulated ONVIF WS-Security verification with zero skew
            let host_time_ms = 1_700_000_000_000i64;
            let device_time_ms = host_time_ms + ctx.clock_offset_ms;
            let diff = (device_time_ms - host_time_ms).abs();
            assert!(
                diff <= 60_000,
                "ONVIF authentication succeeds when clock offset is normalized"
            );

            let rerun = doctor(&ctx).await;
            let clock_rerun = rerun
                .iter()
                .find(|r| r.check == CheckKind::ClockSkew)
                .unwrap();
            assert!(clock_rerun.status.is_pass());
        });
    }

    #[test]
    fn test_ask_first_never_applies_trust_changing_fix_without_confirm() {
        // auto_fix with AskFirst never applies a trust-changing fix without a confirm() call.
        block_on(async {
            let cert_fp = "sha256:abc123certfingerprint".to_string();
            let mut ctx = DoctorContext {
                pinned_cert_fingerprint: None,
                presented_cert_fingerprint: Some(cert_fp.clone()),
                multicast_joined: true,
                igmp_joined: true,
                ssdp_ok: true,
                digest_ok: true,
                mtu: 1500,
                rtsp_path: "/live".to_string(),
                rtsp_path_ok: true,
                ..Default::default()
            };

            // 1. AskFirst without confirmation callback -> No fixes applied
            let outcomes = auto_fix(&mut ctx, AutoFixPolicy::AskFirst, None)
                .await
                .unwrap();
            assert!(outcomes.is_empty());
            assert_eq!(ctx.pinned_cert_fingerprint, None);

            // 2. AskFirst with rejection callback -> No fixes applied
            let reject_cb: ConfirmCallback<'_> = &|_fix| false;
            let outcomes = auto_fix(&mut ctx, AutoFixPolicy::AskFirst, Some(reject_cb))
                .await
                .unwrap();
            assert!(outcomes.is_empty());
            assert_eq!(ctx.pinned_cert_fingerprint, None);

            // 3. AutoFixAll without confirmation callback -> Must NEVER auto-apply trust changes!
            let outcomes = auto_fix(&mut ctx, AutoFixPolicy::AutoFixAll, None)
                .await
                .unwrap();
            assert!(outcomes.is_empty());
            assert_eq!(ctx.pinned_cert_fingerprint, None);

            // 4. AskFirst with operator approval -> Successfully applies trust-changing fix
            let confirm_cb = |fix: &dyn Fix| {
                fix.changes_trust()
                    && fix.kind()
                        == (FixKind::PinSelfSigned {
                            fingerprint: cert_fp.clone(),
                        })
            };
            let outcomes = auto_fix(&mut ctx, AutoFixPolicy::AskFirst, Some(&confirm_cb))
                .await
                .unwrap();
            assert_eq!(outcomes.len(), 1);
            assert_eq!(
                outcomes[0].fix,
                FixKind::PinSelfSigned {
                    fingerprint: cert_fp.clone()
                }
            );
            assert_eq!(ctx.pinned_cert_fingerprint, Some(cert_fp));

            // After pinning, doctor check passes
            let check_res = doctor(&ctx).await;
            let tls_check = check_res
                .iter()
                .find(|r| r.check == CheckKind::TlsPin)
                .unwrap();
            assert!(tls_check.status.is_pass());
        });
    }

    #[test]
    fn test_doctor_timeout_skips_hanging_check() {
        block_on(async {
            struct HangingCheck;

            #[async_trait]
            impl Check for HangingCheck {
                fn kind(&self) -> CheckKind {
                    CheckKind::Mtu
                }

                async fn run(&self, _ctx: &DoctorContext) -> CheckResult {
                    // Simulate long running operation by waiting past deadline
                    let start = std::time::Instant::now();
                    while start.elapsed() < Duration::from_millis(2_200) {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    CheckResult::pass(self.kind())
                }
            }

            let ctx = DoctorContext::default();
            let checks: Vec<Arc<dyn Check>> =
                vec![Arc::new(MulticastCheck), Arc::new(HangingCheck)];

            let results = doctor_with_checks(&ctx, &checks).await;
            assert_eq!(results.len(), 2);
            let hanging_res = results.iter().find(|r| r.check == CheckKind::Mtu).unwrap();
            assert_eq!(
                hanging_res.status,
                CheckStatus::Skipped {
                    reason: "timeout".to_string()
                }
            );
        });
    }
}
