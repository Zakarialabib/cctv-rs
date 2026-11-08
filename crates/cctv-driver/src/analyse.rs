//! Camera endpoint analyser and reviewable driver proposals.
//!
//! §2.3: An analyser that proposes, never auto-merges.
//! Inspects camera Web-UI JavaScript, HTML, API dumps, and firmware strings,
//! extracting RTSP and HTTP paths, detecting authentication schemes and vendor hints,
//! and synthesizing human-reviewable [`DriverProposal`] structures with evidence.

use std::collections::BTreeSet;

/// Authentication scheme detected on management or streaming endpoints.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuthScheme {
    /// No authentication detected (open endpoint).
    #[default]
    None,
    /// HTTP Basic authentication.
    Basic,
    /// HTTP Digest authentication (RFC 2617 / RFC 7616).
    Digest,
    /// WS-Security UsernameToken authentication.
    WsUsernameToken,
    /// Token-based Bearer / OAuth authentication.
    BearerToken,
    /// Vendor proprietary authentication scheme.
    Custom(String),
}

/// A human-reviewable proposal for creating or extending a camera driver.
///
/// Designed to feed pull requests and never auto-register directly into runtime
/// drivers without human inspection.
#[derive(Debug, Clone, PartialEq)]
pub struct DriverProposal {
    /// Inferred vendor or OEM platform name (e.g., `"Hikvision"`, `"Dahua"`, `"Reolink"`).
    pub vendor_hint: String,
    /// Confidence score between 0.0 and 1.0 based on corroborated evidence hits.
    pub confidence: f32,
    /// Extracted RTSP stream path patterns (e.g., `"/h264Preview_01_main"`, `"/live/ch0"`).
    pub rtsp_paths: Vec<String>,
    /// Extracted HTTP/API endpoint paths (e.g., `"/ISAPI/Streaming/channels"`, `"/cgi-bin/snapshot.cgi"`).
    pub http_paths: Vec<String>,
    /// Inferred authentication scheme.
    pub auth_scheme: AuthScheme,
    /// Direct evidence lines (grep hits, regex captures, file locations) that produced each path.
    pub evidence: Vec<String>,
}

impl DriverProposal {
    /// Render the driver proposal as a Markdown pull-request template body.
    pub fn to_markdown_pr(&self) -> String {
        let mut md = String::new();
        md.push_str("## Camera Driver Proposal\n\n");
        md.push_str(&format!("- **Vendor Hint**: {}\n", self.vendor_hint));
        md.push_str(&format!(
            "- **Confidence**: {:.1}%\n",
            self.confidence * 100.0
        ));
        md.push_str(&format!("- **Auth Scheme**: {:?}\n\n", self.auth_scheme));

        md.push_str("### Candidate RTSP Stream Paths\n\n");
        if self.rtsp_paths.is_empty() {
            md.push_str("_None detected_\n\n");
        } else {
            for path in &self.rtsp_paths {
                md.push_str(&format!("- `{path}`\n"));
            }
            md.push('\n');
        }

        md.push_str("### Candidate HTTP / API Endpoints\n\n");
        if self.http_paths.is_empty() {
            md.push_str("_None detected_\n\n");
        } else {
            for path in &self.http_paths {
                md.push_str(&format!("- `{path}`\n"));
            }
            md.push('\n');
        }

        md.push_str("### Corroborating Evidence\n\n");
        if self.evidence.is_empty() {
            md.push_str("_No evidence provided_\n");
        } else {
            for ev in &self.evidence {
                md.push_str(&format!("- {ev}\n"));
            }
        }

        md
    }
}

/// Web-UI endpoint and firmware artifact analyser.
#[derive(Debug, Default, Clone)]
pub struct WebUiAnalyser;

impl WebUiAnalyser {
    /// Create a new web UI analyser.
    pub fn new() -> Self {
        Self
    }

    /// Analyse web-UI content, JavaScript source, or HTTP payload text.
    pub fn analyse(&self, content: &str) -> DriverProposal {
        let mut rtsp_paths = BTreeSet::new();
        let mut http_paths = BTreeSet::new();
        let mut evidence = Vec::new();

        let mut vendor_hits = std::collections::BTreeMap::<&'static str, usize>::new();
        let mut auth_hits = std::collections::BTreeMap::<AuthScheme, usize>::new();

        for (line_no, line) in content.lines().enumerate() {
            let line_num = line_no + 1;
            let lower = line.to_lowercase();

            // Detect vendor keywords
            if lower.contains("hikvision") || lower.contains("isapi") {
                *vendor_hits.entry("Hikvision").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Hikvision/ISAPI marker"));
            }
            if lower.contains("dahua") || lower.contains("dh-") || lower.contains("dvr/channel") {
                *vendor_hits.entry("Dahua").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Dahua marker"));
            }
            if lower.contains("axis") || lower.contains("vapix") || lower.contains("axis-media") {
                *vendor_hits.entry("Axis").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Axis/Vapix marker"));
            }
            if lower.contains("reolink") || lower.contains("h264preview") {
                *vendor_hits.entry("Reolink").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Reolink marker"));
            }
            if lower.contains("amcrest") {
                *vendor_hits.entry("Amcrest").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Amcrest marker"));
            }
            if lower.contains("uniview") || lower.contains("unicview") {
                *vendor_hits.entry("Uniview").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Uniview marker"));
            }
            if lower.contains("xiongmai") || lower.contains("sofia") || lower.contains("xmeye") {
                *vendor_hits.entry("Xiongmai").or_insert(0) += 1;
                evidence.push(format!("line {line_num}: matches Xiongmai/XMeye marker"));
            }

            // Detect authentication keywords
            if lower.contains("ws-security") || lower.contains("usernametoken") {
                *auth_hits.entry(AuthScheme::WsUsernameToken).or_insert(0) += 1;
                evidence.push(format!(
                    "line {line_num}: WS-Security UsernameToken header found"
                ));
            } else if lower.contains("digest ")
                || lower.contains("auth: digest")
                || lower.contains("qop=\"auth\"")
            {
                *auth_hits.entry(AuthScheme::Digest).or_insert(0) += 1;
                evidence.push(format!("line {line_num}: HTTP Digest authentication found"));
            } else if lower.contains("basic ") || lower.contains("authorization: basic") {
                *auth_hits.entry(AuthScheme::Basic).or_insert(0) += 1;
                evidence.push(format!("line {line_num}: HTTP Basic authentication found"));
            } else if lower.contains("bearer ") || lower.contains("oauth") {
                *auth_hits.entry(AuthScheme::BearerToken).or_insert(0) += 1;
                evidence.push(format!(
                    "line {line_num}: Bearer / OAuth authentication found"
                ));
            }

            // Extract RTSP URIs / paths
            for word in line
                .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '`' || c == ';')
            {
                let clean = word.trim_matches(|c: char| c == ',' || c == ')' || c == '(');
                if let Some(rest) = clean.strip_prefix("rtsp://") {
                    if let Some(pos) = rest.find('/') {
                        let path = rest[pos..].to_string();
                        evidence.push(format!("line {line_num}: RTSP URI found: {clean}"));
                        rtsp_paths.insert(path);
                    }
                } else if clean.starts_with("/live/")
                    || clean.starts_with("/h264Preview")
                    || clean.starts_with("/Streaming/Channels")
                    || clean.starts_with("/cam/realmonitor")
                    || clean.starts_with("/axis-media/media.amp")
                    || clean.starts_with("/video1")
                {
                    evidence.push(format!("line {line_num}: RTSP path pattern found: {clean}"));
                    rtsp_paths.insert(clean.to_string());
                }

                // Extract HTTP endpoints
                if clean.starts_with("/ISAPI/")
                    || clean.starts_with("/cgi-bin/")
                    || clean.starts_with("/api/v1/")
                    || clean.starts_with("/onvif/")
                    || clean.starts_with("/snap.jpg")
                    || clean.starts_with("/snapshot.cgi")
                {
                    evidence.push(format!("line {line_num}: HTTP API endpoint found: {clean}"));
                    http_paths.insert(clean.to_string());
                }
            }
        }

        // Determine vendor with highest frequency
        let mut top_vendor = "Generic".to_string();
        let mut max_hits = 0;
        for (vendor, count) in vendor_hits {
            if count > max_hits {
                max_hits = count;
                top_vendor = vendor.to_string();
            }
        }

        // Determine auth scheme with highest frequency
        let mut top_auth = AuthScheme::None;
        let mut max_auth_hits = 0;
        for (auth, count) in auth_hits {
            if count > max_auth_hits {
                max_auth_hits = count;
                top_auth = auth;
            }
        }

        // Calculate confidence
        let mut score: f32 = 0.0;
        if max_hits > 0 {
            score += 0.4;
        }
        if !rtsp_paths.is_empty() {
            score += 0.3;
        }
        if !http_paths.is_empty() {
            score += 0.2;
        }
        if top_auth != AuthScheme::None {
            score += 0.1;
        }
        let confidence = score.min(1.0);

        DriverProposal {
            vendor_hint: top_vendor,
            confidence,
            rtsp_paths: rtsp_paths.into_iter().collect(),
            http_paths: http_paths.into_iter().collect(),
            auth_scheme: top_auth,
            evidence,
        }
    }
}

/// Convenience function to analyse a web-UI text dump or source code string.
pub fn analyse_web_ui(content: &str) -> DriverProposal {
    WebUiAnalyser::new().analyse(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_of_hikvision_endpoints() {
        let dump = r#"
            // Hikvision web client script
            var streamUrl = "rtsp://192.168.1.64:554/Streaming/Channels/101";
            var snapUrl = "/ISAPI/Streaming/channels/101/picture";
            var authHeader = "Authorization: Digest realm=\"IP Camera\"";
        "#;

        let proposal = analyse_web_ui(dump);
        assert_eq!(proposal.vendor_hint, "Hikvision");
        assert_eq!(proposal.auth_scheme, AuthScheme::Digest);
        assert!(proposal
            .rtsp_paths
            .contains(&"/Streaming/Channels/101".to_string()));
        assert!(proposal
            .http_paths
            .contains(&"/ISAPI/Streaming/channels/101/picture".to_string()));
        assert!(proposal.confidence >= 0.7);
        assert!(!proposal.evidence.is_empty());

        let pr = proposal.to_markdown_pr();
        assert!(pr.contains("Camera Driver Proposal"));
        assert!(pr.contains("Hikvision"));
        assert!(pr.contains("/Streaming/Channels/101"));
    }

    #[test]
    fn extraction_of_reolink_endpoints() {
        let dump = r#"
            const preview = "/h264Preview_01_main";
            const snap = "/cgi-bin/snapshot.cgi";
        "#;

        let proposal = analyse_web_ui(dump);
        assert_eq!(proposal.vendor_hint, "Reolink");
        assert!(proposal
            .rtsp_paths
            .contains(&"/h264Preview_01_main".to_string()));
        assert!(proposal
            .http_paths
            .contains(&"/cgi-bin/snapshot.cgi".to_string()));
    }
}
