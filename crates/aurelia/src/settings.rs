//! Preferences for this installation (not per account), kept in
//! `~/.config/aurelia/settings.json` and applied the moment they change.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

use crate::theme::{self, Appearance};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    /// Follow the desktop's light/dark preference.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    /// Page transitions, hover lift and eased state changes.
    pub animations: bool,
    /// Ask "Who's watching?" at launch when several accounts are saved.
    pub profile_picker: bool,
    /// Home's slideshow moves on by itself.
    pub hero_autoplay: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::Dark,
            animations: true,
            profile_picker: true,
            hero_autoplay: true,
        }
    }
}

impl Settings {
    fn file(dir: &Path) -> PathBuf {
        dir.join("settings.json")
    }

    pub fn load(dir: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_else(|err| {
            tracing::warn!("ignoring unreadable settings: {err}");
            Self::default()
        })
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        let tmp = dir.join(".settings.json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, Self::file(dir))
    }
}

/// The settings in use, and where they're saved.
pub struct SettingsStore {
    dir: PathBuf,
    settings: Settings,
}

impl Global for SettingsStore {}

/// The current settings.
pub fn get(cx: &App) -> &Settings {
    &cx.global::<SettingsStore>().settings
}

/// Loads the settings and applies them (theme, motion). Call after
/// `theme::init`.
pub fn init(dir: PathBuf, cx: &mut App) {
    let settings = Settings::load(&dir);
    cx.set_global(SettingsStore { dir, settings });
    apply(cx);
}

/// Changes the settings, saves them and applies the change everywhere.
pub fn update(cx: &mut App, change: impl FnOnce(&mut Settings)) {
    let store = cx.global_mut::<SettingsStore>();
    change(&mut store.settings);
    if let Err(err) = store.settings.save(&store.dir) {
        tracing::warn!("could not save settings: {err}");
    }
    apply(cx);
}

fn apply(cx: &mut App) {
    let settings = get(cx).clone();
    cx.set_reduce_motion(!settings.animations);
    theme::apply(appearance(&settings, cx.window_appearance()), cx);
    cx.refresh_windows();
}

/// The appearance the settings ask for, given the desktop's.
pub fn appearance(settings: &Settings, system: gpui_kit::WindowAppearance) -> Appearance {
    match settings.theme {
        ThemeChoice::Dark => Appearance::Dark,
        ThemeChoice::Light => Appearance::Light,
        ThemeChoice::System => match system {
            gpui_kit::WindowAppearance::Light | gpui_kit::WindowAppearance::VibrantLight => {
                Appearance::Light
            }
            _ => Appearance::Dark,
        },
    }
}

/// Re-applies a System theme after the desktop switched between light and dark.
pub fn system_appearance_changed(system: gpui_kit::WindowAppearance, cx: &mut App) {
    let settings = get(cx);
    if settings.theme == ThemeChoice::System {
        theme::apply(appearance(settings, system), cx);
        cx.refresh_windows();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing_or_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
        std::fs::write(dir.path().join("settings.json"), "{nope").unwrap();
        assert_eq!(Settings::load(dir.path()), Settings::default());
    }

    #[test]
    fn roundtrip_and_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            theme: ThemeChoice::System,
            animations: false,
            ..Settings::default()
        };
        settings.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()), settings);
        // Settings added later keep their defaults in older files.
        std::fs::write(dir.path().join("settings.json"), r#"{"theme":"Light"}"#).unwrap();
        let loaded = Settings::load(dir.path());
        assert_eq!(loaded.theme, ThemeChoice::Light);
        assert!(loaded.animations && loaded.profile_picker && loaded.hero_autoplay);
    }

    #[test]
    fn system_theme_follows_the_desktop() {
        let system = Settings {
            theme: ThemeChoice::System,
            ..Settings::default()
        };
        use gpui_kit::WindowAppearance as W;
        assert_eq!(appearance(&system, W::Light), Appearance::Light);
        assert_eq!(appearance(&system, W::VibrantDark), Appearance::Dark);
        let dark = Settings::default();
        assert_eq!(appearance(&dark, W::Light), Appearance::Dark);
    }
}
