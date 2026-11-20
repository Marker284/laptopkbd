// SPDX-License-Identifier: GPL-2.0-or-later
//! ~/.config/laptopkbd/config.toml: written by laptopkbd-settings, re-read on SIGHUP.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    /// graphite | silver | midnight | retro | oled | neon
    pub skin: String,
    /// Russian legends on the letter keys.
    pub ru_legends: bool,
    /// Share of the screen height taken by the keyboard (0.50 .. 0.70).
    pub keyboard_height: f32,
    /// "fkeys" = F1..F12, "media" = mute, volume, track, play, brightness on F1..F8.
    pub frow: String,
    /// Hold or latch Tux: keys show what Tux+key does in niri.
    pub tux_hints: bool,
    /// Enter laptop mode by itself when a monitor is connected.
    pub auto_laptop_mode: bool,
    /// Widget texts: "auto" (from $LANG), "en", "ru".
    pub language: String,
    pub trackpad: Trackpad,
    pub widgets: Widgets,
    pub device: Device,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Device {
    /// The built-in panel that becomes the keyboard (niri output name).
    pub output: String,
    /// Touchscreen of that panel (substring of the evdev name); grabbed in laptop mode.
    pub touchscreen: String,
    /// niri transform for the panel in laptop mode: "normal", "90", "180", "270".
    /// The touch mapping follows it.
    pub transform: String,
    /// Shell commands run when laptop mode starts / ends (e.g. pause auto-rotation).
    pub on_enter: String,
    pub on_leave: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Trackpad {
    pub speed: f32,
    pub natural_scroll: bool,
    pub tap_to_click: bool,
    pub gestures: bool,
    /// Share of the screen width (0.30 .. 0.55).
    pub width: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Widgets {
    /// Up to three slots each; battery and workspaces take two.
    /// cpu gpu ram disk battery net clock temps workspaces
    pub left: Vec<String>,
    pub right: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            skin: "graphite".into(),
            ru_legends: true,
            keyboard_height: 0.60,
            frow: "fkeys".into(),
            tux_hints: true,
            auto_laptop_mode: true,
            language: "auto".into(),
            trackpad: Trackpad::default(),
            widgets: Widgets::default(),
            device: Device::default(),
        }
    }
}
impl Default for Device {
    fn default() -> Self {
        Self {
            output: "DSI-1".into(),
            touchscreen: "FocalTech FT8203".into(),
            transform: "270".into(),
            on_enter: String::new(),
            on_leave: String::new(),
        }
    }
}
impl Default for Trackpad {
    fn default() -> Self {
        Self { speed: 1.0, natural_scroll: true, tap_to_click: true, gestures: true, width: 0.42 }
    }
}
impl Default for Widgets {
    fn default() -> Self {
        Self {
            left: vec!["cpu".into(), "gpu".into(), "ram".into()],
            right: vec!["battery".into(), "disk".into()],
        }
    }
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
    base.join("laptopkbd").join("config.toml")
}

impl Config {
    pub fn russian(&self) -> bool {
        match self.language.as_str() {
            "ru" => true,
            "en" => false,
            _ => std::env::var("LANG").map_or(false, |l| l.starts_with("ru")),
        }
    }

    pub fn load() -> Self {
        let mut c: Config = std::fs::read_to_string(path())
            .ok()
            .and_then(|s| toml::from_str(&s).map_err(|e| eprintln!("laptopkbd: config: {e}")).ok())
            .unwrap_or_default();
        c.keyboard_height = c.keyboard_height.clamp(0.45, 0.72);
        c.trackpad.width = c.trackpad.width.clamp(0.25, 0.60);
        c.trackpad.speed = c.trackpad.speed.clamp(0.2, 4.0);
        c
    }
}
