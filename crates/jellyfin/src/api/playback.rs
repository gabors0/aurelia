use reqwest::Method;
use serde::Serialize;
use url::Url;

use crate::{Client, PlaybackInfo, Result, Ticks};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ProgressEvent {
    #[serde(rename = "timeupdate")]
    TimeUpdate,
    #[serde(rename = "pause")]
    Pause,
    #[serde(rename = "unpause")]
    Unpause,
}

/// Body of the `/Sessions/Playing*` reports.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackReport {
    pub item_id: String,
    pub media_source_id: String,
    pub play_session_id: String,
    #[serde(rename = "PositionTicks")]
    pub position: Ticks,
    pub is_paused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_stream_index: Option<i32>,
    /// `-1` means subtitles off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle_stream_index: Option<i32>,
    #[serde(rename = "EventName", skip_serializing_if = "Option::is_none")]
    pub event: Option<ProgressEvent>,
    play_method: &'static str,
    can_seek: bool,
}

impl PlaybackReport {
    pub fn new(
        item_id: impl Into<String>,
        media_source_id: impl Into<String>,
        play_session_id: impl Into<String>,
    ) -> Self {
        Self {
            item_id: item_id.into(),
            media_source_id: media_source_id.into(),
            play_session_id: play_session_id.into(),
            position: Ticks::ZERO,
            is_paused: false,
            audio_stream_index: None,
            subtitle_stream_index: None,
            event: None,
            play_method: "DirectPlay",
            can_seek: true,
        }
    }
}

impl Client {
    /// Media sources plus a `PlaySessionId` to tie the reports together.
    pub async fn playback_info(&self, id: &str) -> Result<PlaybackInfo> {
        let query = [("userId", self.require_user_id()?.to_string())];
        self.post_json(
            &format!("Items/{id}/PlaybackInfo"),
            &query,
            &serde_json::json!({}),
        )
        .await
    }

    pub async fn report_start(&self, report: &PlaybackReport) -> Result<()> {
        self.report("Sessions/Playing", report).await
    }

    pub async fn report_progress(&self, report: &PlaybackReport) -> Result<()> {
        self.report("Sessions/Playing/Progress", report).await
    }

    pub async fn report_stopped(&self, report: &PlaybackReport) -> Result<()> {
        self.report("Sessions/Playing/Stopped", report).await
    }

    async fn report(&self, path: &str, report: &PlaybackReport) -> Result<()> {
        let body = serde_json::to_value(report).expect("report serializes");
        self.send_empty(Method::POST, path, &[], Some(&body)).await
    }

    /// Original file, untouched (direct play). Authenticate with
    /// [`Client::token_header`]; the token is deliberately not in the URL.
    pub fn stream_url(&self, item_id: &str, media_source_id: &str, play_session_id: &str) -> Url {
        let mut url = self.url(&format!("Videos/{item_id}/stream"));
        url.query_pairs_mut()
            .append_pair("static", "true")
            .append_pair("mediaSourceId", media_source_id)
            .append_pair("playSessionId", play_session_id);
        url
    }

    /// An external (sidecar) subtitle file in its native format.
    pub fn subtitle_url(
        &self,
        item_id: &str,
        media_source_id: &str,
        stream_index: i32,
        codec: Option<&str>,
    ) -> Url {
        let format = match codec.map(str::to_ascii_lowercase).as_deref() {
            Some("subrip") | None => "srt".to_string(),
            Some("webvtt") => "vtt".to_string(),
            Some(other) => other.to_string(),
        };
        self.url(&format!(
            "Videos/{item_id}/{media_source_id}/Subtitles/{stream_index}/Stream.{format}"
        ))
    }
}
