#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
}

/// One stream as the server describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamInfo {
    pub kind: TrackKind,
    /// Server-side (container) stream index.
    pub index: i32,
    /// A sidecar file rather than a stream inside the container.
    pub external: bool,
}

/// mpv numbers embedded tracks 1, 2, … per type in container order. Returns
/// `None` for external streams (loaded separately) or unknown indices.
pub fn mpv_track_id(streams: &[StreamInfo], kind: TrackKind, index: i32) -> Option<u32> {
    let mut embedded: Vec<i32> = streams
        .iter()
        .filter(|s| s.kind == kind && !s.external)
        .map(|s| s.index)
        .collect();
    embedded.sort_unstable();
    let position = embedded.iter().position(|i| *i == index)?;
    Some(position as u32 + 1)
}
