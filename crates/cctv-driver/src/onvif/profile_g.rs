//! ONVIF Profile G search, playback, and edge recording retrieval.
//!
//! Profile G models edge storage on IP cameras: searching recorded segments,
//! querying summary information, and requesting RTSP playback URIs to retrieve
//! footage from SD cards after network outages.

use cctv_transport::xml;

use super::error::OnvifError;
use super::soap::envelope;

/// ONVIF Search service namespace.
pub const SEARCH_NS: &str = "http://www.onvif.org/ver10/search/wsdl";
/// ONVIF Playback service namespace.
pub const PLAYBACK_NS: &str = "http://www.onvif.org/ver10/recording/wsdl";

/// Summary of edge recordings available on the camera.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordingSummary {
    /// Total count of recordings.
    pub total_recordings: u32,
    /// Data starting boundary in UTC (ISO 8601).
    pub data_from: Option<String>,
    /// Data ending boundary in UTC (ISO 8601).
    pub data_until: Option<String>,
}

/// Information about a track within an edge recording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInformation {
    /// Track token identifier.
    pub track_token: String,
    /// Track type (e.g. `"Video"`, `"Audio"`).
    pub track_type: String,
    /// Description if available.
    pub description: Option<String>,
}

/// An edge recording item returned from search results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingItem {
    /// Recording token identifier.
    pub recording_token: String,
    /// Start time of the recording in ISO 8601 UTC.
    pub earliest_time: Option<String>,
    /// End time of the recording in ISO 8601 UTC.
    pub latest_time: Option<String>,
    /// Tracks associated with this recording.
    pub tracks: Vec<TrackInformation>,
}

/// RTSP playback URI returned by the playback service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybackUri {
    /// Full RTSP playback URI.
    pub uri: String,
    /// Validity timeout string if specified.
    pub timeout: Option<String>,
}

/// Build `GetRecordingSummary` SOAP body.
pub fn build_get_recording_summary() -> String {
    r#"<tse:GetRecordingSummary xmlns:tse="http://www.onvif.org/ver10/search/wsdl"/>"#.to_string()
}

/// Parse `GetRecordingSummaryResponse`.
pub fn parse_get_recording_summary(doc: &str) -> Result<RecordingSummary, OnvifError> {
    let scope = xml::element_body(doc, "GetRecordingSummaryResponse")
        .or_else(|| xml::element_body(doc, "Summary"))
        .unwrap_or(doc);

    let total = xml::element_text(scope, "NumberRecordings")
        .and_then(|t| t.parse::<u32>().ok())
        .unwrap_or(0);
    let data_from = xml::element_text(scope, "DataFrom");
    let data_until = xml::element_text(scope, "DataUntil");

    Ok(RecordingSummary {
        total_recordings: total,
        data_from,
        data_until,
    })
}

/// Build `FindRecordings` SOAP body to query edge recordings.
pub fn build_find_recordings(
    scope_token: Option<&str>,
    earliest: Option<&str>,
    latest: Option<&str>,
    max_matches: u32,
) -> String {
    let mut xml = String::from(
        r#"<tse:FindRecordings xmlns:tse="http://www.onvif.org/ver10/search/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema"><tse:Scope>"#,
    );

    if let Some(scope) = scope_token {
        xml.push_str(&format!(
            "<tt:IncludedRecordings>{}</tt:IncludedRecordings>",
            xml::escape(scope)
        ));
    }

    xml.push_str("</tse:Scope>");

    if let Some(start) = earliest {
        xml.push_str(&format!(
            "<tse:EarliestRecordingTime>{}</tse:EarliestRecordingTime>",
            xml::escape(start)
        ));
    }
    if let Some(end) = latest {
        xml.push_str(&format!(
            "<tse:LatestRecordingTime>{}</tse:LatestRecordingTime>",
            xml::escape(end)
        ));
    }

    xml.push_str(&format!(
        "<tse:MaxMatches>{}</tse:MaxMatches></tse:FindRecordings>",
        max_matches
    ));

    xml
}

/// Parse search token from `FindRecordingsResponse`.
pub fn parse_find_recordings(doc: &str) -> Result<String, OnvifError> {
    xml::element_text(doc, "SearchToken")
        .ok_or_else(|| OnvifError::Parse("FindRecordingsResponse has no SearchToken".to_string()))
}

/// Build `GetRecordingSearchResults` SOAP body.
pub fn build_get_recording_search_results(search_token: &str, max_results: u32) -> String {
    format!(
        r#"<tse:GetRecordingSearchResults xmlns:tse="http://www.onvif.org/ver10/search/wsdl"><tse:SearchToken>{}</tse:SearchToken><tse:MaxResults>{}</tse:MaxResults></tse:GetRecordingSearchResults>"#,
        xml::escape(search_token),
        max_results
    )
}

/// Parse `GetRecordingSearchResultsResponse` into a list of [`RecordingItem`]s.
pub fn parse_get_recording_search_results(doc: &str) -> Result<Vec<RecordingItem>, OnvifError> {
    let mut items = Vec::new();

    for rec_elem in xml::elements(doc, "RecordingInformation") {
        let rec_body = rec_elem.body;
        let token = xml::element_text(rec_body, "RecordingToken").unwrap_or_default();
        let earliest = xml::element_text(rec_body, "EarliestRecording");
        let latest = xml::element_text(rec_body, "LatestRecording");

        let mut tracks = Vec::new();
        for tr_elem in xml::elements(rec_body, "TrackInformation") {
            let tr_body = tr_elem.body;
            let track_token = xml::element_text(tr_body, "TrackToken").unwrap_or_default();
            let track_type =
                xml::element_text(tr_body, "TrackType").unwrap_or_else(|| "Video".into());
            tracks.push(TrackInformation {
                track_token,
                track_type,
                description: None,
            });
        }

        items.push(RecordingItem {
            recording_token: token,
            earliest_time: earliest,
            latest_time: latest,
            tracks,
        });
    }

    Ok(items)
}

/// Build `GetPlaybackUri` SOAP body.
pub fn build_get_playback_uri(recording_token: &str, stream_setup: &str) -> String {
    format!(
        r#"<trp:GetPlaybackUri xmlns:trp="http://www.onvif.org/ver10/recording/wsdl"><trp:StreamSetup>{}</trp:StreamSetup><trp:RecordingToken>{}</trp:RecordingToken></trp:GetPlaybackUri>"#,
        stream_setup,
        xml::escape(recording_token)
    )
}

/// Parse `GetPlaybackUriResponse`.
pub fn parse_get_playback_uri(doc: &str) -> Result<PlaybackUri, OnvifError> {
    let uri = xml::element_text(doc, "Uri")
        .ok_or_else(|| OnvifError::Parse("GetPlaybackUriResponse has no Uri".to_string()))?;
    let timeout = xml::element_text(doc, "Timeout");

    Ok(PlaybackUri { uri, timeout })
}

/// Envelope wrap for search service requests.
pub fn search_envelope(header: Option<&str>, body: &str) -> String {
    envelope(header, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMMARY_XML: &str = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
        <s:Body>
            <tse:GetRecordingSummaryResponse xmlns:tse="http://www.onvif.org/ver10/search/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema">
                <tse:Summary>
                    <tt:DataFrom>2026-11-01T00:00:00Z</tt:DataFrom>
                    <tt:DataUntil>2026-11-08T23:59:59Z</tt:DataUntil>
                    <tt:NumberRecordings>14</tt:NumberRecordings>
                </tse:Summary>
            </tse:GetRecordingSummaryResponse>
        </s:Body>
    </s:Envelope>"#;

    #[test]
    fn parse_summary() {
        let summary = parse_get_recording_summary(SUMMARY_XML).expect("parsed summary");
        assert_eq!(summary.total_recordings, 14);
        assert_eq!(summary.data_from.as_deref(), Some("2026-11-01T00:00:00Z"));
        assert_eq!(summary.data_until.as_deref(), Some("2026-11-08T23:59:59Z"));
    }

    const FIND_RECORDINGS_REPLY: &str = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
        <s:Body>
            <tse:FindRecordingsResponse xmlns:tse="http://www.onvif.org/ver10/search/wsdl">
                <tse:SearchToken>SearchToken_01</tse:SearchToken>
            </tse:FindRecordingsResponse>
        </s:Body>
    </s:Envelope>"#;

    #[test]
    fn parse_search_token() {
        let token = parse_find_recordings(FIND_RECORDINGS_REPLY).expect("token");
        assert_eq!(token, "SearchToken_01");
    }

    const SEARCH_RESULTS_XML: &str = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
        <s:Body>
            <tse:GetRecordingSearchResultsResponse xmlns:tse="http://www.onvif.org/ver10/search/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema">
                <tse:ResultList>
                    <tt:RecordingInformation>
                        <tt:RecordingToken>RecToken_1</tt:RecordingToken>
                        <tt:EarliestRecording>2026-11-08T10:00:00Z</tt:EarliestRecording>
                        <tt:LatestRecording>2026-11-08T11:00:00Z</tt:LatestRecording>
                        <tt:TrackInformation>
                            <tt:TrackToken>Track_Video_1</tt:TrackToken>
                            <tt:TrackType>Video</tt:TrackType>
                        </tt:TrackInformation>
                    </tt:RecordingInformation>
                </tse:ResultList>
            </tse:GetRecordingSearchResultsResponse>
        </s:Body>
    </s:Envelope>"#;

    #[test]
    fn parse_search_results() {
        let items = parse_get_recording_search_results(SEARCH_RESULTS_XML).expect("search results");
        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.recording_token, "RecToken_1");
        assert_eq!(item.earliest_time.as_deref(), Some("2026-11-08T10:00:00Z"));
        assert_eq!(item.latest_time.as_deref(), Some("2026-11-08T11:00:00Z"));
        assert_eq!(item.tracks.len(), 1);
        assert_eq!(item.tracks[0].track_token, "Track_Video_1");
        assert_eq!(item.tracks[0].track_type, "Video");
    }

    const PLAYBACK_URI_XML: &str = r#"<s:Envelope xmlns:s="http://www.w3.org/2003/05/soap-envelope">
        <s:Body>
            <trp:GetPlaybackUriResponse xmlns:trp="http://www.onvif.org/ver10/recording/wsdl" xmlns:tt="http://www.onvif.org/ver10/schema">
                <trp:Uri>rtsp://192.168.1.64:554/playback?recording=RecToken_1</trp:Uri>
                <trp:Timeout>PT60S</trp:Timeout>
            </trp:GetPlaybackUriResponse>
        </s:Body>
    </s:Envelope>"#;

    #[test]
    fn parse_playback_uri_reply() {
        let pb = parse_get_playback_uri(PLAYBACK_URI_XML).expect("playback uri");
        assert_eq!(
            pb.uri,
            "rtsp://192.168.1.64:554/playback?recording=RecToken_1"
        );
        assert_eq!(pb.timeout.as_deref(), Some("PT60S"));
    }
}
