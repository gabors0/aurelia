//! Options for the embedded mpv core.
//!
//! With `config=yes`, mpv parses the user's `mpv.conf` inside
//! `mpv_initialize`, after (and over) anything set before it. So defaults the
//! user may change go in before init only, and options Aurelia depends on go
//! in before *and* after. Built-in scripts (OSC, console, …) follow their
//! options at runtime; only `load-scripts` (the user's scripts) acts during
//! init alone, so it can only be checked afterwards.

/// Set before `mpv_initialize`; mpv.conf may override them.
pub(crate) const DEFAULTS: &[(&str, &str)] = &[
    // `auto-safe` alone tries Vulkan decoding first and, where that fails,
    // falls back to software instead of trying VA-API.
    ("hwdec", "vaapi,auto-safe"),
    // Direct rendering (the decoder writing straight into GL-mapped
    // buffers) gave black video in 9 of 24 runs under Mesa's llvmpipe, 1 of
    // 20 without it; untested on hardware drivers. Without it, software-
    // decoded frames take one more copy; hardware-decoded ones never use it.
    ("vd-lavc-dr", "no"),
];

/// Set before `mpv_initialize` and again after it.
pub(crate) const REQUIRED: &[(&str, &str)] = &[
    ("vo", "libmpv"),
    // Aurelia draws the controls; mpv's own OSD would duplicate them.
    ("osd-level", "0"),
    ("input-default-bindings", "no"),
    ("input-vo-keyboard", "no"),
    ("ytdl", "no"),
    // Built-in scripts; mpv loads and unloads them as these change. Built-in
    // `auto_profiles` stays on, so mpv.conf's conditional profiles apply.
    ("osc", "no"),
    ("load-stats-overlay", "no"),
    ("load-console", "no"),
    ("load-select", "no"),
    ("load-positioning", "no"),
    ("load-commands", "no"),
    ("load-context-menu", "no"),
    // `yes` would hold the last frame at the end instead of ending the file,
    // and no `end-file` would arrive.
    ("keep-open", "no"),
    ("idle", "yes"),
    ("terminal", "no"),
    ("resume-playback", "no"),
    ("save-position-on-quit", "no"),
];

/// Set before `mpv_initialize` only: they act during it.
pub(crate) const BEFORE_INIT: &[(&str, &str)] = &[("config", "yes"), ("load-scripts", "no")];

/// Options from [`BEFORE_INIT`] that mpv.conf turned back on, which we
/// can't undo after init: reported, not fatal.
pub(crate) fn overridden(get: impl Fn(&str) -> Option<String>) -> Vec<&'static str> {
    BEFORE_INIT
        .iter()
        .filter(|(name, wanted)| *name != "config" && get(name).is_some_and(|v| v != *wanted))
        .map(|(name, _)| *name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[(&str, &str)]) -> Vec<String> {
        list.iter().map(|(n, _)| n.to_string()).collect()
    }

    #[test]
    fn video_output_is_reasserted_after_mpv_conf() {
        assert!(names(REQUIRED).contains(&"vo".into()));
        assert!(names(REQUIRED).contains(&"osd-level".into()));
        assert!(names(REQUIRED).contains(&"keep-open".into()));
        assert!(names(REQUIRED).contains(&"osc".into()));
    }

    #[test]
    fn hwdec_is_only_a_default() {
        assert!(names(DEFAULTS).contains(&"hwdec".into()));
        assert!(!names(REQUIRED).contains(&"hwdec".into()));
    }

    #[test]
    fn conditional_profiles_keep_working() {
        let all: Vec<String> = [DEFAULTS, REQUIRED, BEFORE_INIT]
            .iter()
            .flat_map(|l| names(l))
            .collect();
        assert!(!all.contains(&"load-auto-profiles".into()));
    }

    #[test]
    fn reports_scripts_mpv_conf_enabled() {
        let get = |name: &str| match name {
            "load-scripts" => Some("yes".to_string()),
            "config" => Some("no".to_string()),
            _ => None,
        };
        assert_eq!(overridden(get), vec!["load-scripts"]);
        assert!(overridden(|_| Some("no".into())).is_empty());
    }
}
