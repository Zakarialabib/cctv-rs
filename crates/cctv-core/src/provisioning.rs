//! Provisioning vocabulary and auth envelope.
//!
//! Provides types for onboarding cameras, handling stream profile tokens,
//! validating paths to ensure credentials are never leaked, and wrapping
//! secrets in memory-zeroizing envelopes.

use crate::error::CoreError;
use crate::lease::CameraId;
use crate::prelude::*;
use zeroize::{Zeroize, Zeroizing};

#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// Hardware MAC address represented as 6 octets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    /// Create a new `MacAddress` from 6 bytes.
    pub const fn new(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    /// Access raw octets.
    pub const fn octets(&self) -> [u8; 6] {
        self.0
    }
}

impl core::fmt::Display for MacAddress {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

/// Encrypted envelope carrying camera authentication secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct EncryptedAuthEnvelope {
    /// Ephemeral public key used for secret agreement (Base64 encoded).
    pub ephemeral_pubkey_base64: String,
    /// Nonce/IV used in authenticated encryption (Base64 encoded).
    pub nonce_base64: String,
    /// Ciphertext payload containing encrypted credentials (Base64 encoded).
    pub ciphertext_base64: String,
}

impl EncryptedAuthEnvelope {
    /// Create a new encrypted authentication envelope.
    pub fn new(
        ephemeral_pubkey_base64: impl Into<String>,
        nonce_base64: impl Into<String>,
        ciphertext_base64: impl Into<String>,
    ) -> Self {
        Self {
            ephemeral_pubkey_base64: ephemeral_pubkey_base64.into(),
            nonce_base64: nonce_base64.into(),
            ciphertext_base64: ciphertext_base64.into(),
        }
    }
}

/// Decrypted camera authentication secrets, holding sensitive credentials in zeroizing memory.
#[derive(Clone)]
pub struct DecryptedCameraAuth {
    /// ONVIF or camera device password.
    pub password: Zeroizing<String>,
    /// Ezviz verification code.
    pub verification_code: Zeroizing<String>,
}

impl core::fmt::Debug for DecryptedCameraAuth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DecryptedCameraAuth")
            .field("password", &"***")
            .field("verification_code", &"***")
            .finish()
    }
}

impl Zeroize for DecryptedCameraAuth {
    fn zeroize(&mut self) {
        self.password.zeroize();
        self.verification_code.zeroize();
    }
}

impl DecryptedCameraAuth {
    /// Create a new decrypted authentication container.
    pub fn new(password: impl Into<String>, verification_code: impl Into<String>) -> Self {
        Self {
            password: Zeroizing::new(password.into()),
            verification_code: Zeroizing::new(verification_code.into()),
        }
    }
}

/// Profile tokens and RTSP paths for a camera's streams.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct StreamProfileSet {
    /// Main stream profile token (e.g. from ONVIF GetProfiles).
    pub main_profile_token: String,
    /// Relative or host-independent RTSP path for the main stream.
    pub main_rtsp_path: String,
    /// Sub stream profile token, if supported.
    pub sub_profile_token: Option<String>,
    /// Relative or host-independent RTSP path for the sub stream.
    pub sub_rtsp_path: Option<String>,
    /// HTTP path for snapshot retrieval (e.g. from ONVIF GetSnapshotUri).
    pub snapshot_http_path: Option<String>,
}

impl StreamProfileSet {
    /// Validates that none of the profile paths contain embedded credentials or schemes.
    ///
    /// Rejects any path containing `@` or `://`.
    pub fn validate_no_credentials(&self) -> Result<(), CoreError> {
        let check_path = |path: &str| -> Result<(), CoreError> {
            if path.contains('@') || path.contains("://") {
                Err(CoreError::CredentialsPresentInPath)
            } else {
                Ok(())
            }
        };

        check_path(&self.main_rtsp_path)?;

        if let Some(ref sub) = self.sub_rtsp_path {
            check_path(sub)?;
        }

        if let Some(ref snap) = self.snapshot_http_path {
            check_path(snap)?;
        }

        Ok(())
    }
}

/// Capabilities advertised or detected for a camera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct CameraCapabilitySet {
    /// Maximum concurrent RTSP streams the camera hardware can serve (default 2).
    pub max_concurrent_rtsp_streams: u8,
    /// Whether the camera supports PTZ operations.
    pub supports_ptz: bool,
    /// Whether the camera supports ONVIF profiles.
    pub supports_onvif_profiles: bool,
}

impl Default for CameraCapabilitySet {
    fn default() -> Self {
        Self {
            max_concurrent_rtsp_streams: 2,
            supports_ptz: false,
            supports_onvif_profiles: false,
        }
    }
}

/// Provisioning bundle v1 for enrolling a camera into the fleet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct CameraProvisioningBundleV1 {
    /// Stable camera identifier.
    pub camera_id: CameraId,
    /// Camera vendor / manufacturer.
    pub vendor: String,
    /// Primary IP address.
    pub ip_address: String,
    /// Hardware MAC address, if discovered.
    pub mac_address: Option<MacAddress>,
    /// ONVIF device service XAddr, if available.
    pub onvif_device_xaddr: Option<String>,
    /// Observed clock offset relative to host system in milliseconds.
    pub observed_clock_offset_ms: i64,
    /// Set of stream profile tokens and paths.
    pub streams: StreamProfileSet,
    /// Camera capabilities.
    pub capabilities: CameraCapabilitySet,
    /// Encrypted auth envelope holding secrets.
    pub auth_envelope: EncryptedAuthEnvelope,
    /// Provisioning timestamp in unix milliseconds.
    pub provisioned_at_unix_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_address_display() {
        let mac = MacAddress([0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]);
        assert_eq!(mac.to_string(), "00:1a:2b:3c:4d:5e");
        assert_eq!(mac.octets(), [0x00, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E]);
    }

    #[test]
    fn test_validate_no_credentials_clean() {
        let set = StreamProfileSet {
            main_profile_token: "Profile_1".to_string(),
            main_rtsp_path: "/live/ch0".to_string(),
            sub_profile_token: Some("Profile_2".to_string()),
            sub_rtsp_path: Some("/live/ch1".to_string()),
            snapshot_http_path: Some("/onvif/snapshot".to_string()),
        };
        assert!(set.validate_no_credentials().is_ok());
    }

    #[test]
    fn test_validate_no_credentials_rejects_at() {
        let set = StreamProfileSet {
            main_profile_token: "Profile_1".to_string(),
            main_rtsp_path: "admin:pass@host/live".to_string(),
            sub_profile_token: None,
            sub_rtsp_path: None,
            snapshot_http_path: None,
        };
        assert_eq!(
            set.validate_no_credentials(),
            Err(CoreError::CredentialsPresentInPath)
        );
    }

    #[test]
    fn test_validate_no_credentials_rejects_scheme() {
        let set = StreamProfileSet {
            main_profile_token: "Profile_1".to_string(),
            main_rtsp_path: "/live/ch0".to_string(),
            sub_profile_token: None,
            sub_rtsp_path: Some("rtsp://cam/sub".to_string()),
            snapshot_http_path: None,
        };
        assert_eq!(
            set.validate_no_credentials(),
            Err(CoreError::CredentialsPresentInPath)
        );
    }

    #[test]
    fn test_camera_capability_set_default() {
        let caps = CameraCapabilitySet::default();
        assert_eq!(caps.max_concurrent_rtsp_streams, 2);
        assert!(!caps.supports_ptz);
        assert!(!caps.supports_onvif_profiles);
    }

    #[test]
    fn test_decrypted_camera_auth_zeroizes() {
        let mut auth = DecryptedCameraAuth::new("secret_pw", "ezviz_code");
        assert_eq!(auth.password.as_str(), "secret_pw");
        assert_eq!(auth.verification_code.as_str(), "ezviz_code");
        auth.zeroize();
        assert_eq!(auth.password.as_str(), "");
        assert_eq!(auth.verification_code.as_str(), "");
    }
}
