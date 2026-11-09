use crate::protocol::{Alarm, CLASSIC_RGB, ColorTab};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub brightness: u8,
    pub ic_order: usize,
    pub ic_length: i64,
    // -1 rotates like the original app, otherwise a CLASSIC_RGB index
    pub mic_color: i32
}

impl Default for Settings {
    fn default() -> Self {
        Settings { brightness: 49, ic_order: 0, ic_length: 84, mic_color: -1 }
    }
}

// Segments are stored bottom first; the top segment has no end (it runs to the full length).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub color: usize,
    pub end: Option<i64>
}

pub fn default_segments() -> Vec<Segment> {
    vec![Segment { color: 0, end: Some(42) }, Segment { color: 2, end: None }]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delay {
    pub hour: u8,
    pub minute: u8,
    pub enable: bool
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FavoriteKind {
    #[serde(rename_all = "camelCase")]
    Color {
        color_tab: ColorTab,
        rgb: [u8; 3],
        hue: f64,
        sat: f64,
        cct: u8,
        wb: u8,
        swatch: [u8; 3],
        packets: Vec<Vec<u8>>
    },
    #[serde(rename_all = "camelCase")]
    Mode {
        scene_tab: usize,
        mode_index: usize,
        speed: u8,
        dots: Vec<[u8; 3]>,
        packets: Vec<Vec<u8>>
    },
    #[serde(rename_all = "camelCase")]
    Segments {
        ic_length: i64,
        segments: Vec<Segment>,
        dots: Vec<[u8; 3]>
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub id: String,
    pub name: String,
    pub group: u8,
    pub brightness: u8,
    #[serde(flatten)]
    pub kind: FavoriteKind
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetupLamp {
    pub device: String,
    // favourite id or "off"
    pub action: String
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    pub id: String,
    pub name: String,
    pub lamps: Vec<SetupLamp>
}

// Lamps added before; the browser kept them in its permission store, here they live in the state file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    pub id: String,
    pub ble_name: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub group: Option<u8>,
    #[serde(default)]
    pub shortened: bool
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Store {
    pub settings: Settings,
    pub segments: Vec<Segment>,
    pub alarms: Vec<Alarm>,
    pub delay: Option<Delay>,
    pub favorites: Vec<Favorite>,
    pub setups: Vec<Setup>,
    pub devices: Vec<KnownDevice>
}

impl Default for Store {
    fn default() -> Self {
        Store {
            settings: Settings::default(),
            segments: default_segments(),
            alarms: Vec::new(),
            delay: None,
            favorites: Vec::new(),
            setups: Vec::new(),
            devices: Vec::new()
        }
    }
}

pub fn default_path() -> PathBuf {
    if let Ok(p) = std::env::var("RAINGEL_STATE") {
        return PathBuf::from(p);
    }
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("raingel").join("state.json")
}

impl Store {
    // A broken or missing file falls back to defaults, like localStorage reads in the browser.
    pub fn load(path: &PathBuf) -> Store {
        let mut store: Store =
            std::fs::read_to_string(path).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default();
        let defaults = Settings::default();
        if store.settings.ic_length < 16 {
            store.settings.ic_length = defaults.ic_length;
        }
        if store.settings.brightness > 100 {
            store.settings.brightness = defaults.brightness;
        }
        if store.settings.mic_color < -1 || store.settings.mic_color >= CLASSIC_RGB.len() as i32 {
            store.settings.mic_color = defaults.mic_color;
        }
        if store.segments.len() < 2 {
            store.segments = default_segments();
        }
        store
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("[path={}] Cannot create state dir", dir.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn device(&self, id: &str) -> Option<&KnownDevice> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn device_mut(&mut self, id: &str) -> Option<&mut KnownDevice> {
        self.devices.iter_mut().find(|d| d.id == id)
    }
}

pub fn new_id() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}-{}", now.as_millis(), now.subsec_nanos() % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_favorites() {
        let mut store = Store::default();
        store.favorites.push(Favorite {
            id: "1".into(),
            name: "Mode".into(),
            group: 16,
            brightness: 100,
            kind: FavoriteKind::Mode { scene_tab: 1, mode_index: 2, speed: 3, dots: vec![[1, 2, 3]], packets: vec![vec![7]] }
        });
        let raw = serde_json::to_string(&store).unwrap();
        assert!(raw.contains("\"kind\":\"mode\""));
        assert_eq!(serde_json::from_str::<Store>(&raw).unwrap(), store);
    }

    #[test]
    fn invalid_values_fall_back() {
        let dir = std::env::temp_dir().join(format!("raingel-test-{}", new_id()));
        let path = dir.join("state.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, r#"{"settings":{"brightness":40,"icOrder":0,"icLength":3,"micColor":99},"segments":[]}"#)
            .unwrap();
        let store = Store::load(&path);
        assert_eq!(store.settings.brightness, 40);
        assert_eq!(store.settings.ic_length, 84);
        assert_eq!(store.settings.mic_color, -1);
        assert_eq!(store.segments, default_segments());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
