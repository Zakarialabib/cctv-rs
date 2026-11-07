//! Multi-stream composite recording profiles and layout definitions.
//!
//! A [`RecordingProfile`] specifies how multiple streams (main + sub-streams
//! from the same camera, or streams from adjacent cameras in a group) are
//! combined into a single composite recording file:
//!
//! - `cameras`: list of participating [`CameraId`]s.
//! - `layout`: display geometry ([`GridLayout`]).
//! - `audio`: optional [`AudioSource`] selection.
//! - `container`: target container format ([`RecordingContainer`]: `Mp4`, `Mkv`, `Ts`).
//!
//! [`RecordingProfile::validate_streams`] enforces homogeneous codec constraints
//! across all inputs for containers that cannot coalesce mixed codecs (e.g. H.264 + H.265
//! in an MP4 container).

use crate::prelude::*;

use crate::backend::StreamInfo;
use crate::lease::CameraId;
#[cfg(feature = "serde-sync")]
use serde::{Deserialize, Serialize};

/// Target container format for composite recording files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(rename_all = "lowercase"))]
pub enum RecordingContainer {
    /// MP4 ISOBMFF container (`.mp4`).
    Mp4,
    /// Matroska multimedia container (`.mkv`).
    Mkv,
    /// MPEG Transport Stream container (`.ts`).
    Ts,
}

/// Visual layout configuration for composite camera streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-sync", serde(tag = "type", rename_all = "snake_case"))]
pub enum GridLayout {
    /// Single stream (1x1 full viewport).
    Single,
    /// Two streams side-by-side (1x2).
    SplitHorizontal,
    /// Two streams stacked vertically (2x1).
    SplitVertical,
    /// 2x2 grid layout (up to 4 streams).
    Quad,
    /// 3x3 grid layout (up to 9 streams).
    NineGrid,
    /// Custom rows and columns grid.
    Custom {
        /// Number of grid rows.
        rows: u8,
        /// Number of grid columns.
        cols: u8,
    },
    /// Picture-in-picture layout with main stream and inset stream.
    Pip {
        /// Inset stream index.
        inset_index: usize,
    },
}

impl GridLayout {
    /// Return the maximum number of stream slots supported by this layout.
    pub fn max_streams(&self) -> usize {
        match self {
            Self::Single => 1,
            Self::SplitHorizontal | Self::SplitVertical => 2,
            Self::Quad => 4,
            Self::NineGrid => 9,
            Self::Custom { rows, cols } => (*rows as usize).saturating_mul(*cols as usize),
            Self::Pip { .. } => 2,
        }
    }
}

/// Audio source routing for composite recording.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde-sync",
    serde(tag = "type", content = "source", rename_all = "snake_case")
)]
pub enum AudioSource {
    /// No audio track included.
    Muted,
    /// Audio routed from a specific camera.
    Camera(CameraId),
    /// Audio routed from a specific stream index in the profile.
    StreamIndex(usize),
}

/// Error validating a [`RecordingProfile`] or its assigned streams.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    /// No cameras or streams were provided.
    #[error("profile has empty stream list")]
    EmptyStreams,

    /// Mixed video codecs detected that the target container cannot coalesce.
    #[error("mixed video codecs ({codec_a} vs {codec_b}) cannot be coalesced into container {container:?}")]
    MixedCodecs {
        /// First codec detected.
        codec_a: String,
        /// Conflicting codec detected.
        codec_b: String,
        /// Target container.
        container: RecordingContainer,
    },

    /// The number of streams exceeds the capacity of the chosen layout.
    #[error("layout capacity exceeded: {streams} streams for layout allowing at most {capacity}")]
    LayoutCapacityExceeded {
        /// Stream count provided.
        streams: usize,
        /// Max streams allowed by layout.
        capacity: usize,
    },

    /// Specified audio source is invalid or references an unknown stream/camera.
    #[error("invalid audio source reference: {detail}")]
    InvalidAudioSource {
        /// Explanation of failure.
        detail: String,
    },
}

/// Specification for recording multiple streams into a single composite file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde-sync", derive(Serialize, Deserialize))]
pub struct RecordingProfile {
    /// Participating cameras in the composite recording.
    pub cameras: Vec<CameraId>,
    /// Tile layout for the composite views.
    pub layout: GridLayout,
    /// Selected audio source track.
    pub audio: Option<AudioSource>,
    /// Target file container format.
    pub container: RecordingContainer,
}

impl RecordingProfile {
    /// Create a new recording profile.
    pub fn new(
        cameras: Vec<CameraId>,
        layout: GridLayout,
        audio: Option<AudioSource>,
        container: RecordingContainer,
    ) -> Self {
        Self {
            cameras,
            layout,
            audio,
            container,
        }
    }

    /// Validate the profile against a slice of resolved [`StreamInfo`] objects.
    ///
    /// Checks:
    /// 1. Stream list is non-empty.
    /// 2. Stream count does not exceed layout capacity.
    /// 3. In MP4 containers, mixed codecs (e.g. H.264 + H.265) are rejected.
    /// 4. Audio source index (if `AudioSource::StreamIndex`) is within bounds.
    pub fn validate_streams(&self, streams: &[StreamInfo]) -> Result<(), ProfileError> {
        if streams.is_empty() {
            return Err(ProfileError::EmptyStreams);
        }

        let max_capacity = self.layout.max_streams();
        if streams.len() > max_capacity {
            return Err(ProfileError::LayoutCapacityExceeded {
                streams: streams.len(),
                capacity: max_capacity,
            });
        }

        // Validate audio source
        if let Some(AudioSource::StreamIndex(idx)) = self.audio {
            if idx >= streams.len() {
                return Err(ProfileError::InvalidAudioSource {
                    detail: format!(
                        "audio stream index {idx} out of bounds (have {} streams)",
                        streams.len()
                    ),
                });
            }
        }

        // Validate codec homogeneity for containers that require it (e.g. MP4)
        if self.container == RecordingContainer::Mp4 {
            let mut first_codec: Option<&str> = None;
            for stream in streams {
                if let Some(ref codec) = stream.codec {
                    let normalized = normalize_codec(codec);
                    if let Some(prev) = first_codec {
                        if prev != normalized {
                            return Err(ProfileError::MixedCodecs {
                                codec_a: prev.to_string(),
                                codec_b: normalized.to_string(),
                                container: self.container,
                            });
                        }
                    } else {
                        first_codec = Some(normalized);
                    }
                }
            }
        }

        Ok(())
    }
}

/// Normalizes codec string for comparison (e.g. `AVC1`, `H264` -> `H264`).
fn normalize_codec(c: &str) -> &str {
    let lower = c.as_bytes();
    if lower.eq_ignore_ascii_case(b"h264")
        || lower.eq_ignore_ascii_case(b"avc")
        || lower.eq_ignore_ascii_case(b"avc1")
    {
        "H264"
    } else if lower.eq_ignore_ascii_case(b"h265")
        || lower.eq_ignore_ascii_case(b"hevc")
        || lower.eq_ignore_ascii_case(b"hvc1")
    {
        "H265"
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::StreamQuality;

    fn make_stream(quality: StreamQuality, codec: Option<&str>) -> StreamInfo {
        StreamInfo {
            uri: "rtsp://localhost/stream".to_string(),
            profile: "test".to_string(),
            quality,
            width: Some(1920),
            height: Some(1080),
            codec: codec.map(|s| s.to_string()),
        }
    }

    #[test]
    fn validate_streams_rejects_empty() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam1")],
            GridLayout::Single,
            None,
            RecordingContainer::Mp4,
        );
        assert_eq!(
            profile.validate_streams(&[]),
            Err(ProfileError::EmptyStreams)
        );
    }

    #[test]
    fn validate_streams_rejects_layout_overflow() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam1"), CameraId::new("cam2")],
            GridLayout::Single,
            None,
            RecordingContainer::Mp4,
        );
        let streams = vec![
            make_stream(StreamQuality::Main, Some("H264")),
            make_stream(StreamQuality::Sub, Some("H264")),
        ];
        assert_eq!(
            profile.validate_streams(&streams),
            Err(ProfileError::LayoutCapacityExceeded {
                streams: 2,
                capacity: 1
            })
        );
    }

    #[test]
    fn validate_streams_rejects_mixed_codecs_in_mp4() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam1"), CameraId::new("cam2")],
            GridLayout::Quad,
            None,
            RecordingContainer::Mp4,
        );
        let streams = vec![
            make_stream(StreamQuality::Main, Some("H264")),
            make_stream(StreamQuality::Sub, Some("H265")),
        ];
        assert_eq!(
            profile.validate_streams(&streams),
            Err(ProfileError::MixedCodecs {
                codec_a: "H264".to_string(),
                codec_b: "H265".to_string(),
                container: RecordingContainer::Mp4,
            })
        );
    }

    #[test]
    fn validate_streams_allows_matching_codecs_in_mp4() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam1"), CameraId::new("cam2")],
            GridLayout::Quad,
            None,
            RecordingContainer::Mp4,
        );
        let streams = vec![
            make_stream(StreamQuality::Main, Some("h264")),
            make_stream(StreamQuality::Sub, Some("avc1")),
        ];
        assert!(profile.validate_streams(&streams).is_ok());
    }

    #[test]
    fn validate_streams_checks_audio_source_bounds() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam1")],
            GridLayout::Single,
            Some(AudioSource::StreamIndex(5)),
            RecordingContainer::Mkv,
        );
        let streams = vec![make_stream(StreamQuality::Main, Some("H264"))];
        assert!(matches!(
            profile.validate_streams(&streams),
            Err(ProfileError::InvalidAudioSource { .. })
        ));
    }
}
