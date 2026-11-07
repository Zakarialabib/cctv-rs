//! Multi-stream composite muxer with GOP-caching and timestamp normalization.
//!
//! Reuses the NAL unit parsing and GOP cache mechanism from [`crate::StreamMux`], extending it
//! to $N$ synchronized inputs. Per-input RTP timestamps are translated and normalized
//! to a shared, monotonically increasing common reference clock (milliseconds).

use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use cctv_core::{CameraId, ProfileError, RecordingProfile, StreamInfo};

use crate::stream_mux::{classify_nalu, GopCache, Nalu, NaluType, VideoCodec};

/// An input stream configuration for the composite muxer.
#[derive(Debug, Clone)]
pub struct CompositeInputConfig {
    /// Associated camera identifier.
    pub camera_id: CameraId,
    /// Stream index in the composite layout.
    pub stream_index: usize,
    /// Expected video codec.
    pub codec: VideoCodec,
    /// Clock rate in Hz (e.g. 90,000 for standard H.264 / H.265 RTP).
    pub clock_rate: u32,
}

impl CompositeInputConfig {
    /// Create a new input stream configuration.
    pub fn new(camera_id: CameraId, stream_index: usize, codec: VideoCodec) -> Self {
        Self {
            camera_id,
            stream_index,
            codec,
            clock_rate: 90_000,
        }
    }
}

/// A composite multiplexed NAL unit tagged with its input source and normalized presentation timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeNalu {
    /// Source camera ID.
    pub camera_id: CameraId,
    /// Input index in the composite grid/layout.
    pub stream_index: usize,
    /// Presentation timestamp on the normalized reference timeline (milliseconds).
    pub normalized_pts_ms: u64,
    /// Parsed NAL unit payload.
    pub nalu: Nalu,
}

/// Tracks and normalizes input-specific timestamps to a common timeline.
#[derive(Debug, Clone)]
struct TimestampNormalizer {
    clock_rate: u32,
    first_rtp_ts: Option<u32>,
    first_common_pts_ms: Option<u64>,
    last_normalized_pts_ms: u64,
}

impl TimestampNormalizer {
    fn new(clock_rate: u32) -> Self {
        Self {
            clock_rate: if clock_rate == 0 { 90_000 } else { clock_rate },
            first_rtp_ts: None,
            first_common_pts_ms: None,
            last_normalized_pts_ms: 0,
        }
    }

    /// Normalizes a raw input RTP timestamp to the common timeline.
    ///
    /// If `common_reference_pts_ms` is provided on first frame, anchors this input
    /// to the shared common timeline so all streams align.
    fn normalize(&mut self, raw_rtp_ts: u32, common_reference_pts_ms: u64) -> u64 {
        let first_raw = *self.first_rtp_ts.get_or_insert(raw_rtp_ts);
        let base_pts = *self
            .first_common_pts_ms
            .get_or_insert(common_reference_pts_ms);

        // Account for 32-bit RTP timestamp wraparound
        let delta_ticks = raw_rtp_ts.wrapping_sub(first_raw);
        let delta_ms = (delta_ticks as u64).saturating_mul(1_000) / (self.clock_rate as u64);

        let normalized = base_pts.saturating_add(delta_ms);
        // Ensure monotonicity
        let monotonic = normalized.max(self.last_normalized_pts_ms);
        self.last_normalized_pts_ms = monotonic;
        monotonic
    }
}

/// Multi-stream composite muxer managing $N$ inputs with GOP caching and timestamp normalization.
#[derive(Debug)]
pub struct CompositeStreamMux {
    profile: RecordingProfile,
    inputs: Vec<CompositeInputConfig>,
    gop_caches: Vec<Arc<RwLock<GopCache>>>,
    normalizers: Arc<RwLock<Vec<TimestampNormalizer>>>,
    sender: broadcast::Sender<CompositeNalu>,
}

impl CompositeStreamMux {
    /// Create a new `CompositeStreamMux` from a validated [`RecordingProfile`] and input configs.
    pub fn new(
        profile: RecordingProfile,
        inputs: Vec<CompositeInputConfig>,
    ) -> Result<Self, ProfileError> {
        let (sender, _) = broadcast::channel(1024);
        let mut gop_caches = Vec::with_capacity(inputs.len());
        let mut normalizers = Vec::with_capacity(inputs.len());

        for input in &inputs {
            gop_caches.push(Arc::new(RwLock::new(GopCache::new())));
            normalizers.push(TimestampNormalizer::new(input.clock_rate));
        }

        Ok(Self {
            profile,
            inputs,
            gop_caches,
            normalizers: Arc::new(RwLock::new(normalizers)),
            sender,
        })
    }

    /// Build a `CompositeStreamMux` by validating the [`RecordingProfile`] against resolved [`StreamInfo`]s.
    pub fn from_profile_and_streams(
        profile: RecordingProfile,
        streams: &[StreamInfo],
    ) -> Result<Self, ProfileError> {
        profile.validate_streams(streams)?;

        let mut inputs = Vec::with_capacity(streams.len());
        for (idx, stream) in streams.iter().enumerate() {
            let cam_id = if idx < profile.cameras.len() {
                profile.cameras[idx].clone()
            } else {
                CameraId::new(format!("camera-{}", idx))
            };

            let codec = match stream.codec.as_deref() {
                Some(c) if c.eq_ignore_ascii_case("h265") || c.eq_ignore_ascii_case("hevc") => {
                    VideoCodec::H265
                }
                _ => VideoCodec::H264,
            };

            inputs.push(CompositeInputConfig::new(cam_id, idx, codec));
        }

        Self::new(profile, inputs)
    }

    /// The associated recording profile.
    pub fn profile(&self) -> &RecordingProfile {
        &self.profile
    }

    /// Subscribe to the composite stream channel.
    pub fn subscribe(&self) -> broadcast::Receiver<CompositeNalu> {
        self.sender.subscribe()
    }

    /// Read the snapshot GOP cache for a given stream index.
    pub async fn gop_cache(&self, stream_index: usize) -> Option<GopCache> {
        if let Some(cache_lock) = self.gop_caches.get(stream_index) {
            Some(cache_lock.read().await.clone())
        } else {
            None
        }
    }

    /// Ingest an access unit for a specific stream index with its raw RTP timestamp
    /// and a global/common reference timestamp (e.g. wall clock ms).
    pub async fn ingest_access_unit(
        &self,
        stream_index: usize,
        raw_rtp_ts: u32,
        common_now_ms: u64,
        nal_units: Vec<Vec<u8>>,
    ) {
        let Some(input_cfg) = self.inputs.get(stream_index) else {
            return;
        };
        let Some(cache_lock) = self.gop_caches.get(stream_index) else {
            return;
        };

        let normalized_pts = {
            let mut norm_guard = self.normalizers.write().await;
            if let Some(norm) = norm_guard.get_mut(stream_index) {
                norm.normalize(raw_rtp_ts, common_now_ms)
            } else {
                common_now_ms
            }
        };

        let mut cache = cache_lock.write().await;
        let mut is_idr_frame = false;
        let parsed_nals: Vec<Nalu> = nal_units
            .into_iter()
            .map(|data| {
                let nalu_type = classify_nalu(input_cfg.codec, &data);
                if nalu_type == NaluType::IdrKeyframe {
                    is_idr_frame = true;
                }
                Nalu {
                    codec: input_cfg.codec,
                    nalu_type,
                    data,
                }
            })
            .collect();

        if is_idr_frame {
            cache.clear_idr();
        }

        for nalu in parsed_nals {
            cache.update(&nalu);
            let comp_nalu = CompositeNalu {
                camera_id: input_cfg.camera_id.clone(),
                stream_index,
                normalized_pts_ms: normalized_pts,
                nalu,
            };
            let _ = self.sender.send(comp_nalu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cctv_core::{GridLayout, RecordingContainer, StreamQuality};

    #[tokio::test]
    async fn composite_muxer_normalizes_timestamps_across_inputs() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam-01"), CameraId::new("cam-02")],
            GridLayout::SplitHorizontal,
            None,
            RecordingContainer::Mkv,
        );

        let inputs = vec![
            CompositeInputConfig::new(CameraId::new("cam-01"), 0, VideoCodec::H264),
            CompositeInputConfig::new(CameraId::new("cam-02"), 1, VideoCodec::H264),
        ];

        let mux = CompositeStreamMux::new(profile, inputs).expect("mux created");
        let mut rx = mux.subscribe();

        // Feed input 0 at base 90_000 raw RTP, common time 1000 ms
        mux.ingest_access_unit(0, 90_000, 1000, vec![vec![0x67, 0x42, 0x00]])
            .await;

        let nalu0 = rx.recv().await.expect("received nalu0");
        assert_eq!(nalu0.stream_index, 0);
        assert_eq!(nalu0.normalized_pts_ms, 1000);

        // Feed input 1 with arbitrary raw RTP (e.g. 500_000) at common time 1050 ms
        mux.ingest_access_unit(1, 500_000, 1050, vec![vec![0x67, 0x42, 0x00]])
            .await;

        let nalu1 = rx.recv().await.expect("received nalu1");
        assert_eq!(nalu1.stream_index, 1);
        assert_eq!(nalu1.normalized_pts_ms, 1050);

        // Feed input 0 +3000 ticks (3000 / 90 = 33.33ms -> ~33ms)
        mux.ingest_access_unit(0, 93_000, 1033, vec![vec![0x65, 0x88]])
            .await;

        let nalu0_next = rx.recv().await.expect("received nalu0 next");
        assert_eq!(nalu0_next.stream_index, 0);
        assert_eq!(nalu0_next.normalized_pts_ms, 1033);
    }

    #[test]
    fn composite_muxer_rejects_incompatible_streams() {
        let profile = RecordingProfile::new(
            vec![CameraId::new("cam-01"), CameraId::new("cam-02")],
            GridLayout::SplitHorizontal,
            None,
            RecordingContainer::Mp4,
        );

        let streams = vec![
            StreamInfo {
                uri: "rtsp://localhost/main".to_string(),
                profile: "main".to_string(),
                quality: StreamQuality::Main,
                width: Some(1920),
                height: Some(1080),
                codec: Some("H264".to_string()),
            },
            StreamInfo {
                uri: "rtsp://localhost/sub".to_string(),
                profile: "sub".to_string(),
                quality: StreamQuality::Sub,
                width: Some(640),
                height: Some(360),
                codec: Some("H265".to_string()),
            },
        ];

        let err = CompositeStreamMux::from_profile_and_streams(profile, &streams).unwrap_err();
        assert!(matches!(err, ProfileError::MixedCodecs { .. }));
    }
}
