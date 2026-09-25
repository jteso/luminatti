//! User-wide desktop appearance, shared across repositories.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use gpui::Rgba;
use serde::{Deserialize, Serialize};

static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn activate(self) {
        ACTIVE_THEME.store(u8::from(self == Self::Light), Ordering::Relaxed);
    }

    fn active() -> Self {
        if ACTIVE_THEME.load(Ordering::Relaxed) == 1 {
            Self::Light
        } else {
            Self::Dark
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub(super) struct UserProfile {
    pub theme: Theme,
}

impl UserProfile {
    pub fn load() -> Self {
        profile_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|contents| serde_json::from_str(&contents).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> io::Result<()> {
        let path = profile_path().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "user data directory is unavailable")
        })?;
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let temporary_path = path.with_extension("tmp");
        fs::write(&temporary_path, serde_json::to_vec_pretty(self).map_err(io::Error::other)?)?;
        fs::rename(temporary_path, path)
    }
}

fn profile_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|directory| directory.join("luminatti").join("user-profile.json"))
}

/// All desktop surfaces use this color entry point so an appearance change
/// reaches menus, diffs, status views, and text together.
pub(super) fn rgb(hex: u32) -> Rgba {
    gpui::rgb(if Theme::active() == Theme::Light {
        light_color(hex)
    } else {
        hex
    })
}

pub(super) fn rgba(hex: u32) -> Rgba {
    let color = if Theme::active() == Theme::Light {
        if hex >> 8 == 0x141820 {
            0x263242 // translucent scrim behind the comment dialog
        } else {
            light_color(hex >> 8)
        }
    } else {
        hex >> 8
    };
    gpui::rgba((color << 8) | (hex & 0xff))
}

fn light_color(hex: u32) -> u32 {
    match hex {
        0x282d36 => 0xf8fafc, // canvas
        0x2f3540 => 0xf0f3f7, // sidebar and panels
        0x3b4350 => 0xd9e0e8, // borders
        0xc9d1d9 => 0x243142, // primary text
        0x9da7b6 => 0x627083, // secondary text
        0x79c99e => 0x287b51,
        0xf18c96 => 0xb6414c,
        0xe5b567 => 0x94621d,
        0x7aa2f7 => 0x326dc2,
        0x2b3a54 => 0xe5efff,
        0x3d4654 => 0xd0d9e4,
        0x384863 => 0xdce9fa,
        0x405271 => 0xd0e1f8,
        0x101720 => 0xffffff, // text on filled action buttons
        _ => {
            let [_, red, green, blue] = hex.to_be_bytes();
            let maximum = red.max(green).max(blue);
            let minimum = red.min(green).min(blue);
            if maximum <= 0x70 {
                // Existing dark surface shades become related light shades.
                let lift = |channel: u8| -> u32 {
                    (253_i32 - (maximum as i32 - 28).max(0) / 3
                        - (maximum as i32 - channel as i32) / 5)
                        .clamp(0, 255) as u32
                };
                (lift(red) << 16) | (lift(green) << 8) | lift(blue)
            } else if maximum - minimum < 45 {
                if maximum > 165 { 0x344153 } else { 0x627083 }
            } else {
                // Keep semantic and syntax hues while giving them contrast on white.
                let scale = 0.74;
                let dim = |channel: u8| -> u32 { (f32::from(channel) * scale) as u32 };
                (dim(red) << 16) | (dim(green) << 8) | dim(blue)
            }
        }
    }
}
