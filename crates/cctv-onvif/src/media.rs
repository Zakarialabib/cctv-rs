//! Media service (`ver10/media/wsdl`): profiles, stream URIs, snapshot URIs and
//! the encoder write path.
//!
//! Ported from Simple-Signage `protocol/media.rs` + `media_parse.rs`.  Stream
//! URIs must be *enumerated*, never guessed: `/stream1` is one vendor's
//! convention, while others use `/axis-media/media.amp`,
//! `/Streaming/Channels/101` or `/cam/realmonitor?channel=1&subtype=0`, and the
//! low-bandwidth sub-stream is only discoverable through `GetProfiles`.
//!
//! Builders return SOAP **body** fragments; [`crate::soap::envelope`] wraps
//! them.  Parsers are prefix-agnostic.

use cctv_transport::xml;

use crate::error::OnvifError;

/// A media profile as reported by the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaProfile {
    /// Opaque token addressing this profile in later calls (including PTZ).
    pub token: String,
    /// Human-readable name, e.g. `MainStream`.
    pub name: String,
    /// Encoder name if the profile carries a video encoder configuration.
    pub encoding: Option<String>,
    /// Advertised width.
    pub width: Option<u32>,
    /// Advertised height.
    pub height: Option<u32>,
    /// Advertised frame-rate limit.
    pub fps: Option<u32>,
    /// True when a PTZ configuration is attached to this profile.
    pub ptz_capable: bool,
}

impl MediaProfile {
    /// Heuristic: is this the low-bandwidth sub-stream?  ONVIF has no flag for
    /// it; sub-streams are conventionally named.
    pub fn is_substream(&self) -> bool {
        let n = self.name.to_ascii_lowercase();
        n.contains("sub") || n.contains("second") || n.contains("low") || n.contains("mobile")
    }

    /// Pixel count, when a resolution is advertised.
    pub fn pixels(&self) -> Option<u64> {
        match (self.width, self.height) {
            (Some(w), Some(h)) => Some(u64::from(w) * u64::from(h)),
            _ => None,
        }
    }
}

/// `GetProfiles` body.
pub fn build_get_profiles() -> &'static str {
    "<trt:GetProfiles/>"
}

/// `GetStreamUri` body: RTP-Unicast over UDP, the universally supported
/// baseline (older devices refuse TCP / HTTP tunnelling outright).
pub fn build_get_stream_uri(profile_token: &str) -> String {
    format!(
        "<trt:GetStreamUri><trt:StreamSetup><tt:Stream>RTP-Unicast</tt:Stream><tt:Transport><tt:Protocol>UDP</tt:Protocol></tt:Transport></trt:StreamSetup><trt:ProfileToken>{}</trt:ProfileToken></trt:GetStreamUri>",
        xml::escape(profile_token)
    )
}

/// `GetSnapshotUri` body.
pub fn build_get_snapshot_uri(profile_token: &str) -> String {
    format!(
        "<trt:GetSnapshotUri><trt:ProfileToken>{}</trt:ProfileToken></trt:GetSnapshotUri>",
        xml::escape(profile_token)
    )
}

/// `SetVideoEncoderConfiguration` body.
///
/// `ForcePersistence=false` is deliberate: persisting writes the camera's
/// NVRAM and silently reconfigures any NVR sharing the stream; a session-scoped
/// change is recoverable by a power cycle.
pub fn build_set_video_encoder_config(
    configuration_token: &str,
    encoding: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> String {
    format!(
        "<trt:SetVideoEncoderConfiguration><trt:Configuration token=\"{}\"><tt:Encoding>{}</tt:Encoding><tt:Resolution><tt:Width>{width}</tt:Width><tt:Height>{height}</tt:Height></tt:Resolution><tt:RateControl><tt:FrameRateLimit>{fps}</tt:FrameRateLimit></tt:RateControl></trt:Configuration><trt:ForcePersistence>false</trt:ForcePersistence></trt:SetVideoEncoderConfiguration>",
        xml::escape(configuration_token),
        xml::escape(encoding)
    )
}

fn profile_spans(body: &str) -> Vec<(usize, usize, bool)> {
    // Devices emit `Profiles` (the schema name) or `Profile`; accept both.
    let mut out = Vec::new();
    let mut cursor = 0;
    loop {
        let next = ["Profiles", "Profile"]
            .iter()
            .filter_map(|n| xml::find_element(body, n, cursor))
            .min_by_key(|s| s.start);
        let Some(span) = next else { break };
        out.push((span.start, span.open_end, span.self_closing));
        cursor = span.open_end;
    }
    out
}

/// Parse a `GetProfilesResponse`.
///
/// The profile token is an **attribute**; a profile without one is dropped,
/// since every later call needs it.  Each profile's fields are read only from
/// its own body, so nothing leaks from one profile into the next, and
/// `Width` / `Height` are read only inside `Resolution`.
pub fn parse_profiles(body: &str) -> Vec<MediaProfile> {
    let spans = profile_spans(body);
    let mut profiles = Vec::new();
    for (i, &(start, open_end, self_closing)) in spans.iter().enumerate() {
        let tag = &body[start..open_end];
        let token = xml::attribute(tag, "token").unwrap_or_default();
        let end = spans.get(i + 1).map(|s| s.0).unwrap_or(body.len());
        let scope = if self_closing { "" } else { &body[open_end..end] };
        let res = xml::element_body(scope, "Resolution").unwrap_or("");
        let profile = MediaProfile {
            token,
            name: xml::element_text(scope, "Name").unwrap_or_default(),
            encoding: xml::element_text(scope, "Encoding"),
            width: xml::element_text(res, "Width").and_then(|v| v.parse().ok()),
            height: xml::element_text(res, "Height").and_then(|v| v.parse().ok()),
            fps: xml::element_text(scope, "FrameRateLimit").and_then(|v| parse_fps(&v)),
            ptz_capable: xml::find_element(scope, "PTZConfiguration", 0).is_some(),
        };
        if !profile.token.is_empty() {
            profiles.push(profile);
        }
    }
    profiles
}

/// Frame rate as an integer or a float (`"25"`, `"12.5"`); floats truncate.
fn parse_fps(raw: &str) -> Option<u32> {
    let t = raw.trim();
    t.parse::<u32>()
        .ok()
        .or_else(|| t.parse::<f64>().ok().map(|f| f.max(0.0) as u32))
}

/// The `VideoEncoderConfiguration` token of one profile (what
/// `SetVideoEncoderConfiguration` addresses — not the profile token).
pub fn encoder_config_token(body: &str, profile_token: &str) -> Option<String> {
    let spans = profile_spans(body);
    for (i, &(start, open_end, self_closing)) in spans.iter().enumerate() {
        if self_closing || xml::attribute(&body[start..open_end], "token").as_deref() != Some(profile_token) {
            continue;
        }
        let end = spans.get(i + 1).map(|s| s.0).unwrap_or(body.len());
        let scope = &body[open_end..end];
        let span = xml::find_element(scope, "VideoEncoderConfiguration", 0)?;
        return xml::attribute(&scope[span.start..span.open_end], "token");
    }
    None
}

/// Parse a `GetStreamUriResponse` or `GetSnapshotUriResponse`: the URI at
/// `MediaUri/Uri`, falling back to a top-level `Uri` for firmware that omits
/// the wrapper.  An empty or missing URI is an error, never `Ok("")`.
pub fn parse_media_uri(body: &str) -> Result<String, OnvifError> {
    let scope = xml::element_body(body, "MediaUri")
        .filter(|s| !s.is_empty())
        .unwrap_or(body);
    match xml::element_text(scope, "Uri") {
        Some(uri) => Ok(uri),
        None => Err(OnvifError::Parse("reply contained no MediaUri/Uri".into())),
    }
}

/// The best full-quality profile: largest resolution, non-substream names
/// winning ties.  `None` only for an empty list.
pub fn pick_main_profile(profiles: &[MediaProfile]) -> Option<&MediaProfile> {
    profiles
        .iter()
        .max_by_key(|p| (p.pixels().unwrap_or(0), !p.is_substream()))
}

/// The best low-bandwidth profile: smallest resolution, substream names
/// winning ties.  Falls back to the only profile.
pub fn pick_substream_profile(profiles: &[MediaProfile]) -> Option<&MediaProfile> {
    profiles
        .iter()
        .min_by_key(|p| (p.pixels().unwrap_or(u64::MAX), !p.is_substream()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AXIS_PROFILES: &str = r#"<SOAP-ENV:Envelope xmlns:SOAP-ENV="http://www.w3.org/2003/05/soap-envelope">
  <SOAP-ENV:Body><trt:GetProfilesResponse>
      <trt:Profiles token="profile_1" fixed="true">
        <tt:Name>MainStream</tt:Name>
        <tt:VideoEncoderConfiguration token="venc_1">
          <tt:Name>VideoEncoder_1</tt:Name><tt:Encoding>H264</tt:Encoding>
          <tt:Resolution><tt:Width>1920</tt:Width><tt:Height>1080</tt:Height></tt:Resolution>
          <tt:RateControl><tt:FrameRateLimit>30</tt:FrameRateLimit></tt:RateControl>
        </tt:VideoEncoderConfiguration>
        <tt:PTZConfiguration token="ptz_1"><tt:Name>PTZ_1</tt:Name></tt:PTZConfiguration>
      </trt:Profiles>
      <trt:Profiles token="profile_2" fixed="true">
        <tt:Name>SubStream</tt:Name>
        <tt:VideoEncoderConfiguration token="venc_2"><tt:Encoding>H264</tt:Encoding>
          <tt:Resolution><tt:Width>640</tt:Width><tt:Height>360</tt:Height></tt:Resolution>
          <tt:RateControl><tt:FrameRateLimit>15.0</tt:FrameRateLimit></tt:RateControl>
        </tt:VideoEncoderConfiguration>
      </trt:Profiles>
  </trt:GetProfilesResponse></SOAP-ENV:Body></SOAP-ENV:Envelope>"#;

    #[test]
    fn parses_two_profiles_with_tokens_names_and_resolutions() {
        let p = parse_profiles(AXIS_PROFILES);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].token, "profile_1");
        assert_eq!(p[0].name, "MainStream");
        assert_eq!(p[0].encoding.as_deref(), Some("H264"));
        assert_eq!((p[0].width, p[0].height, p[0].fps), (Some(1920), Some(1080), Some(30)));
        assert!(p[0].ptz_capable);
        assert_eq!((p[1].width, p[1].height), (Some(640), Some(360)));
        assert!(!p[1].ptz_capable, "must not inherit the main profile's PTZ");
    }

    #[test]
    fn a_float_frame_rate_is_truncated_not_discarded() {
        assert_eq!(parse_profiles(AXIS_PROFILES)[1].fps, Some(15));
    }

    #[test]
    fn fields_are_not_leaked_from_one_profile_into_the_next() {
        let xml = r#"<GetProfilesResponse><Profiles token="a"><Name>MainStream</Name></Profiles><Profiles token="b"></Profiles></GetProfilesResponse>"#;
        let p = parse_profiles(xml);
        assert_eq!(p[1].name, "");
    }

    #[test]
    fn width_outside_resolution_is_ignored() {
        let xml = r#"<GetProfilesResponse><Profiles token="p1"><Name>Main</Name>
          <VideoSourceConfiguration><Bounds x="0" y="0" width="99" height="99"/></VideoSourceConfiguration>
          <VideoEncoderConfiguration><Resolution><Width>2560</Width><Height>1440</Height></Resolution></VideoEncoderConfiguration>
          </Profiles></GetProfilesResponse>"#;
        let p = parse_profiles(xml);
        assert_eq!((p[0].width, p[0].height), (Some(2560), Some(1440)));
    }

    #[test]
    fn unprefixed_singular_and_tokenless_profiles_are_handled() {
        let xml = r#"<GetProfilesResponse><Profile token="only"><Name>Solo</Name></Profile><Profiles><Name>Broken</Name></Profiles></GetProfilesResponse>"#;
        let p = parse_profiles(xml);
        assert_eq!(p.len(), 1, "the tokenless profile is dropped");
        assert_eq!(p[0].token, "only");
        assert!(parse_profiles("<GetProfilesResponse></GetProfilesResponse>").is_empty());
    }

    #[test]
    fn escaped_characters_in_a_token_are_decoded() {
        let xml = r#"<GetProfilesResponse><Profiles token="a&amp;b"><Name>Amp &lt;test&gt;</Name></Profiles></GetProfilesResponse>"#;
        let p = parse_profiles(xml);
        assert_eq!(p[0].token, "a&b");
        assert_eq!(p[0].name, "Amp <test>");
    }

    #[test]
    fn stream_and_snapshot_uris_parse_from_media_uri() {
        let xml = r#"<trt:GetStreamUriResponse><trt:MediaUri><tt:Uri>rtsp://192.168.1.64:554/Streaming/Channels/101</tt:Uri><tt:Timeout>PT60S</tt:Timeout></trt:MediaUri></trt:GetStreamUriResponse>"#;
        assert_eq!(parse_media_uri(xml).unwrap(), "rtsp://192.168.1.64:554/Streaming/Channels/101");
        let dahua = "<MediaUri><Uri>rtsp://h/cam/realmonitor?channel=1&amp;subtype=0</Uri></MediaUri>";
        assert_eq!(parse_media_uri(dahua).unwrap(), "rtsp://h/cam/realmonitor?channel=1&subtype=0");
        let decoy = "<E><X><Uri>http://decoy</Uri></X><MediaUri><Uri>rtsp://real/s</Uri></MediaUri></E>";
        assert_eq!(parse_media_uri(decoy).unwrap(), "rtsp://real/s");
        assert!(parse_media_uri("<GetStreamUriResponse/>").is_err());
        assert!(parse_media_uri("<MediaUri><Uri>   </Uri></MediaUri>").is_err());
    }

    #[test]
    fn main_and_substream_are_selected_by_resolution_then_name() {
        let p = parse_profiles(AXIS_PROFILES);
        assert_eq!(pick_main_profile(&p).unwrap().token, "profile_1");
        assert_eq!(pick_substream_profile(&p).unwrap().token, "profile_2");
        let named = parse_profiles(r#"<R><Profiles token="a"><Name>MainStream</Name></Profiles><Profiles token="b"><Name>SubStream</Name></Profiles></R>"#);
        assert_eq!(pick_main_profile(&named).unwrap().token, "a");
        assert_eq!(pick_substream_profile(&named).unwrap().token, "b");
        assert!(pick_main_profile(&[]).is_none());
    }

    #[test]
    fn request_builders_escape_tokens_and_never_persist() {
        let s = build_get_stream_uri("a&b<c>");
        assert!(s.contains("a&amp;b&lt;c&gt;") && s.contains("RTP-Unicast") && s.contains("<tt:Protocol>UDP</tt:Protocol>"));
        assert!(build_get_snapshot_uri("t").contains("<trt:ProfileToken>t</trt:ProfileToken>"));
        let set = build_set_video_encoder_config("a&b", "H265", 3840, 2160, 30);
        assert!(set.contains("token=\"a&amp;b\"") && set.contains("<tt:Width>3840</tt:Width>"));
        assert!(set.contains("<trt:ForcePersistence>false</trt:ForcePersistence>"));
    }

    #[test]
    fn the_encoder_token_is_scoped_to_the_requested_profile() {
        assert_eq!(encoder_config_token(AXIS_PROFILES, "profile_2").as_deref(), Some("venc_2"));
        assert_eq!(encoder_config_token(AXIS_PROFILES, "profile_1").as_deref(), Some("venc_1"));
        assert!(encoder_config_token(AXIS_PROFILES, "nope").is_none());
    }
}
