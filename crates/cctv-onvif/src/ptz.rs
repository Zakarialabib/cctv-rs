//! PTZ service (`ver20/ptz/wsdl`): moves, stop and presets.
//!
//! Ported from Simple-Signage `protocol/onvif.rs` (ContinuousMove,
//! AbsoluteMove, Stop, GotoPreset); `GetPresets`, `SetPreset` and
//! `RemovePreset` are added from the ONVIF PTZ service specification.
//!
//! Coordinates use the generic spaces: velocity in `-1..=1` per axis;
//! absolute pan / tilt in `-1..=1` and zoom in `0..=1`
//! (`PositionGenericSpace`).

use cctv_core::Preset;
use cctv_transport::xml;

const VELOCITY_PT: &str = "http://www.onvif.org/ver10/tptz/PanTiltSpaces/VelocityGenericSpace";
const VELOCITY_Z: &str = "http://www.onvif.org/ver10/tptz/ZoomSpaces/VelocityGenericSpace";
const POSITION_PT: &str = "http://www.onvif.org/ver10/tptz/PanTiltSpaces/PositionGenericSpace";
const POSITION_Z: &str = "http://www.onvif.org/ver10/tptz/ZoomSpaces/PositionGenericSpace";

fn clamp_unit(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// `ContinuousMove` body.  Velocities are clamped to `-1..=1`; the timeout
/// defaults to 30 s, after which the camera stops on its own.
pub fn build_continuous_move(profile_token: &str, pan: f32, tilt: f32, zoom: f32, timeout_secs: Option<u64>) -> String {
    format!(
        "<tptz:ContinuousMove><tptz:ProfileToken>{}</tptz:ProfileToken><tptz:Velocity><tt:PanTilt x=\"{}\" y=\"{}\" space=\"{VELOCITY_PT}\"/><tt:Zoom x=\"{}\" space=\"{VELOCITY_Z}\"/></tptz:Velocity><tptz:Timeout>PT{}S</tptz:Timeout></tptz:ContinuousMove>",
        xml::escape(profile_token),
        clamp_unit(pan),
        clamp_unit(tilt),
        clamp_unit(zoom),
        timeout_secs.unwrap_or(30)
    )
}

/// `AbsoluteMove` body in the generic position space.
pub fn build_absolute_move(profile_token: &str, pan: f32, tilt: f32, zoom: f32) -> String {
    format!(
        "<tptz:AbsoluteMove><tptz:ProfileToken>{}</tptz:ProfileToken><tptz:Position><tt:PanTilt x=\"{}\" y=\"{}\" space=\"{POSITION_PT}\"/><tt:Zoom x=\"{}\" space=\"{POSITION_Z}\"/></tptz:Position></tptz:AbsoluteMove>",
        xml::escape(profile_token),
        clamp_unit(pan),
        clamp_unit(tilt),
        if zoom.is_finite() { zoom.clamp(0.0, 1.0) } else { 0.0 }
    )
}

/// `Stop` body.
pub fn build_stop(profile_token: &str, pan_tilt: bool, zoom: bool) -> String {
    format!(
        "<tptz:Stop><tptz:ProfileToken>{}</tptz:ProfileToken><tptz:PanTilt>{pan_tilt}</tptz:PanTilt><tptz:Zoom>{zoom}</tptz:Zoom></tptz:Stop>",
        xml::escape(profile_token)
    )
}

/// `GotoPreset` body.
pub fn build_goto_preset(profile_token: &str, preset_token: &str) -> String {
    format!(
        "<tptz:GotoPreset><tptz:ProfileToken>{}</tptz:ProfileToken><tptz:PresetToken>{}</tptz:PresetToken></tptz:GotoPreset>",
        xml::escape(profile_token),
        xml::escape(preset_token)
    )
}

/// `GetPresets` body.
pub fn build_get_presets(profile_token: &str) -> String {
    format!(
        "<tptz:GetPresets><tptz:ProfileToken>{}</tptz:ProfileToken></tptz:GetPresets>",
        xml::escape(profile_token)
    )
}

/// `SetPreset` body: stores the current position under `name`, overwriting
/// `preset_token` when given.
pub fn build_set_preset(profile_token: &str, name: &str, preset_token: Option<&str>) -> String {
    let token = preset_token
        .map(|t| format!("<tptz:PresetToken>{}</tptz:PresetToken>", xml::escape(t)))
        .unwrap_or_default();
    format!(
        "<tptz:SetPreset><tptz:ProfileToken>{}</tptz:ProfileToken><tptz:PresetName>{}</tptz:PresetName>{token}</tptz:SetPreset>",
        xml::escape(profile_token),
        xml::escape(name)
    )
}

/// `RemovePreset` body.
pub fn build_remove_preset(profile_token: &str, preset_token: &str) -> String {
    format!(
        "<tptz:RemovePreset><tptz:ProfileToken>{}</tptz:ProfileToken><tptz:PresetToken>{}</tptz:PresetToken></tptz:RemovePreset>",
        xml::escape(profile_token),
        xml::escape(preset_token)
    )
}

fn attr_f32(tag_scope: &str, element: &str, attr: &str) -> Option<f32> {
    let span = xml::find_element(tag_scope, element, 0)?;
    xml::attribute(&tag_scope[span.start..span.open_end], attr)?.parse().ok()
}

/// Parse a `GetPresetsResponse` into device-model presets.  Presets without
/// a token are dropped; a missing position reads as zero.
pub fn parse_presets(body: &str) -> Vec<Preset> {
    xml::elements(body, "Preset")
        .into_iter()
        .filter_map(|p| {
            let id = p.attr("token")?;
            let position = xml::element_body(p.body, "PTZPosition").unwrap_or("");
            Some(Preset {
                name: xml::element_text(p.body, "Name").unwrap_or_else(|| id.clone()),
                pan: attr_f32(position, "PanTilt", "x").unwrap_or(0.0),
                tilt: attr_f32(position, "PanTilt", "y").unwrap_or(0.0),
                zoom: attr_f32(position, "Zoom", "x").unwrap_or(0.0),
                config: serde_json::Value::Null,
                id,
            })
        })
        .collect()
}

/// The `PresetToken` of a `SetPresetResponse`.
pub fn parse_set_preset(body: &str) -> Option<String> {
    xml::element_text(body, "PresetToken")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_move_carries_token_velocity_and_timeout() {
        let s = build_continuous_move("token-123", 0.5, -0.3, 0.1, None);
        assert!(s.contains("<tptz:ProfileToken>token-123</tptz:ProfileToken>"));
        assert!(s.contains("x=\"0.5\" y=\"-0.3\""));
        assert!(s.contains("<tt:Zoom x=\"0.1\""));
        assert!(s.contains("PT30S"));
        assert!(build_continuous_move("t", 0.0, 0.0, 0.0, Some(15)).contains("PT15S"));
    }

    #[test]
    fn velocities_are_clamped_and_nan_is_zeroed() {
        let s = build_continuous_move("t", 2.0, -7.0, f32::NAN, None);
        assert!(s.contains("x=\"1\" y=\"-1\""), "{s}");
        assert!(s.contains("<tt:Zoom x=\"0\""), "{s}");
    }

    #[test]
    fn absolute_move_uses_the_position_space() {
        let s = build_absolute_move("tok", 0.25, -0.5, 0.75);
        assert!(s.contains("AbsoluteMove") && s.contains("PositionGenericSpace"));
        assert!(s.contains("x=\"0.25\" y=\"-0.5\"") && s.contains("<tt:Zoom x=\"0.75\""));
    }

    #[test]
    fn stop_and_goto_carry_their_flags_and_tokens() {
        let s = build_stop("pt", true, false);
        assert!(s.contains("<tptz:PanTilt>true</tptz:PanTilt>") && s.contains("<tptz:Zoom>false</tptz:Zoom>"));
        let g = build_goto_preset("profile-1", "preset-5");
        assert!(g.contains("profile-1") && g.contains("<tptz:PresetToken>preset-5</tptz:PresetToken>"));
    }

    #[test]
    fn set_preset_overwrites_only_when_a_token_is_given() {
        assert!(!build_set_preset("p", "Door", None).contains("PresetToken"));
        let s = build_set_preset("p", "D&R", Some("7"));
        assert!(s.contains("<tptz:PresetName>D&amp;R</tptz:PresetName><tptz:PresetToken>7</tptz:PresetToken>"));
        assert!(build_remove_preset("p", "7").contains("RemovePreset"));
    }

    #[test]
    fn presets_parse_with_positions() {
        let body = r#"<tptz:GetPresetsResponse>
          <tptz:Preset token="1"><tt:Name>Door</tt:Name><tt:PTZPosition>
            <tt:PanTilt x="0.5" y="-0.25" space="x"/><tt:Zoom x="0.1" space="z"/></tt:PTZPosition></tptz:Preset>
          <tptz:Preset token="2"><tt:Name>Gate</tt:Name></tptz:Preset>
          <tptz:Preset><tt:Name>no token</tt:Name></tptz:Preset>
        </tptz:GetPresetsResponse>"#;
        let p = parse_presets(body);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].id.as_str(), p[0].name.as_str()), ("1", "Door"));
        assert_eq!((p[0].pan, p[0].tilt, p[0].zoom), (0.5, -0.25, 0.1));
        assert_eq!((p[1].pan, p[1].tilt, p[1].zoom), (0.0, 0.0, 0.0));
        assert_eq!(parse_set_preset("<SetPresetResponse><PresetToken>9</PresetToken></SetPresetResponse>").as_deref(), Some("9"));
    }
}
