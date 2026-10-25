//! The quirks database: model- and firmware-specific camera defects, as data.
//!
//! A camera's control surface changes with firmware, and the changes are not
//! announced anywhere a client can read.  The response is a versioned table keyed
//! by `(vendor, model prefix, firmware prefix)` that a driver consults **before**
//! it tries something known to fail on that device.
//!
//! Keeping this in TOML rather than `match` arms has two consequences that
//! matter: a new quirk is a data change anyone can submit and review, and the
//! table can be shipped newer than the crate.
//!
//! Ported from `tv-rs`'s quirks table — same shape, same two rules: order is
//! precedence, and a row without a source is a guess that does not ship.
//!
//! ```no_run
//! use cctv_core::quirks::Quirks;
//!
//! let quirks = Quirks::bundled().expect("the bundled table parses");
//! if let Some(quirk) = quirks.lookup("acme", Some("PTZ-9000"), Some("V5.3")) {
//!     println!("{} -> {}", quirk.issue, quirk.action);
//! }
//! ```
//!
//! The table is behind the `quirks` feature, which pulls the TOML parser.  A
//! build that only needs the device model does not pay for it.

use std::fmt;

/// One documented defect and its documented handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quirk {
    /// Vendor the row applies to (`acme`, `ezviz`, …), lowercase.
    pub vendor: String,
    /// Only apply when the model starts with this. `None` matches every model.
    pub model_prefix: Option<String>,
    /// Only apply when the firmware starts with this. `None` matches every version.
    pub firmware_prefix: Option<String>,
    /// Stable identifier of the defect, e.g. `rtsp_tcp_refused`.
    pub issue: String,
    /// What the driver should do instead, e.g. `rtsp_udp_only`.
    pub action: String,
    /// Why this row exists, in one sentence — the part a contributor can check.
    pub note: Option<String>,
}

impl Quirk {
    /// Whether this row applies to a device.
    #[must_use]
    pub fn matches(&self, vendor: &str, model: Option<&str>, firmware: Option<&str>) -> bool {
        if !self.vendor.eq_ignore_ascii_case(vendor) {
            return false;
        }
        if let Some(prefix) = &self.model_prefix {
            match model {
                Some(model) if model.starts_with(prefix.as_str()) => {}
                _ => return false,
            }
        }
        if let Some(prefix) = &self.firmware_prefix {
            match firmware {
                Some(firmware) if firmware.starts_with(prefix.as_str()) => {}
                _ => return false,
            }
        }
        true
    }
}

/// What a driver should do differently, decoded from a row's `action`.
///
/// The `action` strings in the table are a fixed vocabulary rather than prose: a
/// typo would otherwise turn a workaround into a comment nothing reads.  An
/// unrecognised action is preserved as [`QuirkAction::Other`] so the table can
/// gain vocabulary before the code does, and a test asserts that every action
/// shipped in `quirks/default.toml` decodes to a known variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuirkAction {
    /// Skip straight to this port and never fall back: the other one is dead on
    /// this firmware, so probing it costs a timeout per command.
    StartAtPort(u16),
    /// The device refuses RTSP over TCP / interleaved; stream over UDP.
    UseUdpTransport,
    /// The device will refuse commands until a setting is enabled; surface
    /// [`crate::CameraError::AuthFailed`] rather than retrying.
    ExpectAuthRequired,
    /// Trust the capability flags in the probe response over the model name.
    TrustDeviceInfoFlags,
    /// The main stream fails on this firmware; fall back to the sub stream.
    RetryWithSubstream,
    /// The operator has to change something in the camera's settings.
    PromptUser,
    /// Vocabulary the crate does not know yet.
    Other(String),
}

impl QuirkAction {
    /// Decodes one `action` string.
    #[must_use]
    pub fn decode(action: &str) -> Self {
        let action = action.trim();
        match action {
            "rtsp_udp_only" => Self::UseUdpTransport,
            "auth_not_permitted" => Self::ExpectAuthRequired,
            "read_probe_flags" => Self::TrustDeviceInfoFlags,
            "retry_substream" => Self::RetryWithSubstream,
            "point_user_at_settings" => Self::PromptUser,
            other => match other.strip_prefix("start_at_") {
                Some(port) => port
                    .parse::<u16>()
                    .map_or_else(|_| Self::Other(other.to_string()), Self::StartAtPort),
                None => Self::Other(other.to_string()),
            },
        }
    }

    /// The port this action pins, if any.
    #[must_use]
    pub fn port(&self) -> Option<u16> {
        match self {
            Self::StartAtPort(port) => Some(*port),
            _ => None,
        }
    }
}

impl fmt::Display for QuirkAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartAtPort(port) => write!(f, "start at {port}"),
            Self::UseUdpTransport => f.write_str("stream over UDP"),
            Self::ExpectAuthRequired => f.write_str("expect auth refusal"),
            Self::TrustDeviceInfoFlags => f.write_str("trust probe flags"),
            Self::RetryWithSubstream => f.write_str("retry on the sub stream"),
            Self::PromptUser => f.write_str("prompt the user"),
            Self::Other(action) => write!(f, "{action}"),
        }
    }
}

/// Why a table could not be loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuirkError {
    /// The TOML itself is invalid.
    Parse(String),
    /// A row is missing a field it cannot be acted on without.
    MissingField {
        /// Zero-based index of the row in the file.
        row: usize,
        /// The field that is absent or empty.
        field: &'static str,
    },
}

impl fmt::Display for QuirkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => write!(f, "quirks table is not valid TOML: {message}"),
            Self::MissingField { row, field } => {
                write!(f, "quirks row {row} has no `{field}`")
            }
        }
    }
}

impl std::error::Error for QuirkError {}

/// The loaded table.
#[derive(Debug, Clone, Default)]
pub struct Quirks {
    entries: Vec<Quirk>,
}

impl Quirks {
    /// Parses a TOML table.
    ///
    /// # Errors
    ///
    /// Returns [`QuirkError::Parse`] on invalid TOML and
    /// [`QuirkError::MissingField`] when a row lacks `vendor`, `issue` or
    /// `action` — a row without those cannot be acted on, so accepting it would
    /// silently disable the workaround it was written for.
    pub fn parse(source: &str) -> Result<Self, QuirkError> {
        let document: toml::Value = source
            .parse()
            .map_err(|error: toml::de::Error| QuirkError::Parse(error.to_string()))?;

        let rows = document
            .get("quirk")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[]);

        let mut entries = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let string = |key: &str| -> Option<String> {
                row.get(key)
                    .and_then(toml::Value::as_str)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            };
            let required = |key: &'static str| -> Result<String, QuirkError> {
                string(key).ok_or(QuirkError::MissingField {
                    row: index,
                    field: key,
                })
            };

            entries.push(Quirk {
                vendor: required("vendor")?.to_ascii_lowercase(),
                model_prefix: string("model_prefix"),
                firmware_prefix: string("firmware_prefix"),
                issue: required("issue")?,
                action: required("action")?,
                note: string("note"),
            });
        }

        Ok(Self { entries })
    }

    /// Parses the table shipped with this crate (`quirks/default.toml`).
    ///
    /// # Errors
    ///
    /// Returns a parse error only if the bundled file is broken, which this
    /// crate's own tests prevent.
    pub fn bundled() -> Result<Self, QuirkError> {
        Self::parse(include_str!("../quirks/default.toml"))
    }

    /// The rows, in file order.
    #[must_use]
    pub fn entries(&self) -> &[Quirk] {
        &self.entries
    }

    /// The first row that applies, or `None`.
    ///
    /// First match wins, so specific rows must be listed before general ones —
    /// the order in the file is the precedence order, and the file says so.
    #[must_use]
    pub fn lookup(
        &self,
        vendor: &str,
        model: Option<&str>,
        firmware: Option<&str>,
    ) -> Option<&Quirk> {
        self.entries
            .iter()
            .find(|quirk| quirk.matches(vendor, model, firmware))
    }

    /// The decoded action for a device, or `None` when no row applies.
    ///
    /// This is the function a driver calls: [`Quirks::lookup`] returns the row
    /// for diagnostics, this returns the thing to *do*.
    #[must_use]
    pub fn action_for(
        &self,
        vendor: &str,
        model: Option<&str>,
        firmware: Option<&str>,
    ) -> Option<QuirkAction> {
        self.lookup(vendor, model, firmware)
            .map(|quirk| QuirkAction::decode(&quirk.action))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = r#"
[[quirk]]
vendor = "ACME"
model_prefix = "PTZ-9"
firmware_prefix = "V5.3"
issue = "rtsp_tcp_refused"
action = "rtsp_udp_only"
note = "V5.3.x firmwares drop the interleaved channel."

[[quirk]]
vendor = "acme"
model_prefix = "PTZ-9"
issue = "rtsp_tcp_refused"
action = "rtsp_udp_only"

[[quirk]]
vendor = "acme"
issue = "auth_not_permitted"
action = "auth_not_permitted"
"#;

    fn table() -> Quirks {
        Quirks::parse(TABLE).expect("the table parses")
    }

    #[test]
    fn the_bundled_table_parses() {
        // The shipped file must always load, even when it holds no rows.
        assert!(Quirks::bundled().is_ok());
    }

    #[test]
    fn a_row_is_read_field_by_field() {
        let quirks = table();
        let first = &quirks.entries()[0];
        assert_eq!(first.vendor, "acme", "vendors are normalised to lowercase");
        assert_eq!(first.model_prefix.as_deref(), Some("PTZ-9"));
        assert_eq!(first.firmware_prefix.as_deref(), Some("V5.3"));
        assert_eq!(first.issue, "rtsp_tcp_refused");
        assert_eq!(first.action, "rtsp_udp_only");
        assert!(first.note.is_some());
        // An omitted optional field stays absent rather than becoming "".
        assert_eq!(quirks.entries()[1].note, None);
    }

    #[test]
    fn matching_is_by_prefix_and_case_insensitive_on_the_vendor() {
        // Only the two PTZ-9 rows: a catch-all row would match everything and
        // make the prefix assertions meaningless.
        let quirks = Quirks::parse(
            r#"
[[quirk]]
vendor = "ACME"
model_prefix = "PTZ-9"
firmware_prefix = "V5.3"
issue = "rtsp_tcp_refused"
action = "rtsp_udp_only"

[[quirk]]
vendor = "acme"
model_prefix = "PTZ-9"
issue = "rtsp_tcp_refused"
action = "rtsp_udp_only"
"#,
        )
        .expect("the table parses");

        assert!(quirks
            .lookup("ACME", Some("PTZ-9000"), Some("V5.3.1 build 2"))
            .is_some());
        // A model outside the prefix does not match.
        assert!(quirks.lookup("acme", Some("C6N"), None).is_none());
        // A firmware outside the prefix falls through to the firmware-less row.
        assert_eq!(
            quirks
                .lookup("acme", Some("PTZ-9"), Some("V5.4"))
                .expect("a row")
                .firmware_prefix,
            None
        );
        // The vendor comparison is case-insensitive on both sides.
        assert!(quirks.lookup("AcMe", Some("PTZ-9"), None).is_some());
    }

    #[test]
    fn order_is_precedence() {
        let quirks = table();
        // Both PTZ-9 rows match; the firmware-scoped one is listed first and wins.
        let hit = quirks
            .lookup("acme", Some("PTZ-9000"), Some("V5.3.1"))
            .expect("a row");
        assert_eq!(hit.firmware_prefix.as_deref(), Some("V5.3"));
        // With no firmware reported, only the general row can match.
        let general = quirks
            .lookup("acme", Some("PTZ-9000"), None)
            .expect("a row");
        assert_eq!(general.firmware_prefix, None);
    }

    #[test]
    fn a_row_with_no_prefixes_matches_every_device_of_its_vendor() {
        let quirks = table();
        let hit = quirks.lookup("acme", Some("anything"), Some("V9")).unwrap();
        assert_eq!(hit.issue, "auth_not_permitted");
    }

    #[test]
    fn an_unknown_vendor_matches_nothing() {
        assert!(table().lookup("other", None, None).is_none());
        assert!(Quirks::default().lookup("acme", None, None).is_none());
    }

    #[test]
    fn the_action_vocabulary_decodes() {
        assert_eq!(
            QuirkAction::decode("rtsp_udp_only"),
            QuirkAction::UseUdpTransport
        );
        assert_eq!(
            QuirkAction::decode("start_at_8554"),
            QuirkAction::StartAtPort(8554)
        );
        assert_eq!(QuirkAction::decode("start_at_8554").port(), Some(8554));
        assert_eq!(
            QuirkAction::decode("retry_substream"),
            QuirkAction::RetryWithSubstream
        );
        assert_eq!(
            QuirkAction::decode("read_probe_flags"),
            QuirkAction::TrustDeviceInfoFlags
        );
        assert_eq!(
            QuirkAction::decode("point_user_at_settings"),
            QuirkAction::PromptUser
        );
        assert_eq!(
            QuirkAction::decode("auth_not_permitted"),
            QuirkAction::ExpectAuthRequired
        );
        // Unknown prose is preserved rather than dropped, so the table can gain
        // vocabulary before the code does.
        assert_eq!(
            QuirkAction::decode("something_new"),
            QuirkAction::Other("something_new".into())
        );
        // A malformed port is not a silent `StartAtPort(0)`.
        assert_eq!(
            QuirkAction::decode("start_at_85x4"),
            QuirkAction::Other("start_at_85x4".into())
        );
        assert_eq!(QuirkAction::UseUdpTransport.port(), None);
    }

    #[test]
    fn every_shipped_action_decodes_to_a_known_variant() {
        // The table is empty today; this test is the gate that keeps a
        // mistyped action out of it the moment a row lands.
        let quirks = Quirks::bundled().expect("bundled");
        for quirk in quirks.entries() {
            assert!(
                !matches!(QuirkAction::decode(&quirk.action), QuirkAction::Other(_)),
                "row {} has action {:?}, which is not in the vocabulary",
                quirk.issue,
                quirk.action
            );
        }
    }

    #[test]
    fn action_for_returns_the_thing_to_do() {
        let quirks = table();
        assert_eq!(
            quirks.action_for("acme", Some("PTZ-9000"), Some("V5.3.1")),
            Some(QuirkAction::UseUdpTransport)
        );
        // The catch-all row catches a model no specific row names.
        assert_eq!(
            quirks.action_for("acme", Some("C6N"), None),
            Some(QuirkAction::ExpectAuthRequired)
        );
        assert_eq!(quirks.action_for("other", Some("PTZ-9"), None), None);
    }

    #[test]
    fn a_row_missing_a_required_field_is_refused() {
        let no_vendor = r#"
[[quirk]]
issue = "x"
action = "y"
"#;
        assert_eq!(
            Quirks::parse(no_vendor).unwrap_err(),
            QuirkError::MissingField {
                row: 0,
                field: "vendor"
            }
        );

        let blank_action = r#"
[[quirk]]
vendor = "acme"
issue = "x"
action = "   "
"#;
        assert_eq!(
            Quirks::parse(blank_action).unwrap_err(),
            QuirkError::MissingField {
                row: 0,
                field: "action"
            }
        );
    }

    #[test]
    fn invalid_toml_is_a_parse_error() {
        assert!(matches!(
            Quirks::parse("[[quirk]\nvendor = "),
            Err(QuirkError::Parse(_))
        ));
    }

    #[test]
    fn a_table_with_no_rows_is_valid() {
        let quirks = Quirks::parse("# nothing yet\n").expect("valid");
        assert!(quirks.entries().is_empty());
        assert!(quirks.lookup("acme", None, None).is_none());
    }

    #[test]
    fn errors_describe_themselves() {
        assert_eq!(
            QuirkError::MissingField {
                row: 2,
                field: "issue"
            }
            .to_string(),
            "quirks row 2 has no `issue`"
        );
        assert!(QuirkError::Parse("bad".into()).to_string().contains("bad"));
    }
}
