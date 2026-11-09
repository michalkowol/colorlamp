// Protocol reverse-engineered from Raingel_release_C55_V202601071930.apk
// (am.doit.dohome.strip.service.StripManager / StripBleManager, bean.Mode).
use btleplug::api::bleuuid::uuid_from_u16;
use uuid::Uuid;

pub const UUID_SCAN: Uuid = uuid_from_u16(0xaf30); // advertised service, used only for scanning
pub const UUID_SERVICE: Uuid = uuid_from_u16(0xae30); // GATT service with the write characteristic
pub const UUID_WRITE: Uuid = uuid_from_u16(0xae01); // write without response
pub const COMPANY_ID: u16 = 22872; // BuildConfig.APP_ID, manufacturer data = [group, type]

pub const DEFAULT_GROUP: u8 = 16;
pub const SEGMENT_PAUSE_MS: u64 = 3000;
pub const MIN_LEDS: i64 = 16;
pub const MAX_LEDS: i64 = 2048;

pub const GROUPS: [(u8, &str); 12] = [
    (1, "Dimmer"),
    (2, "CCT"),
    (3, "RGB"),
    (4, "RGBW"),
    (5, "RGB + CCT (RGBC)"),
    (6, "RGBCW"),
    (16, "Magic (addressable)"),
    (17, "Magic W, 2 paths"),
    (18, "Magic C, 2 paths"),
    (19, "Sphere (motor + laser)"),
    (20, "Magic W"),
    (21, "Magic CW")
];

pub fn group_name(group: u8) -> String {
    match GROUPS.iter().find(|(id, _)| *id == group) {
        Some((_, name)) => name.to_string(),
        None => format!("type {group}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorTab {
    Color,
    Cct,
    Wb,
    Motor,
    Laser,
    Segments
}

impl ColorTab {
    pub fn name(self) -> &'static str {
        match self {
            ColorTab::Color => "Colour",
            ColorTab::Cct => "White temperature",
            ColorTab::Wb => "White",
            ColorTab::Motor => "Motor",
            ColorTab::Laser => "Stars",
            ColorTab::Segments => "Segments"
        }
    }
}

pub fn color_tabs(group: u8) -> &'static [ColorTab] {
    use ColorTab::*;
    match group {
        2 => &[Cct],
        4 => &[Color, Wb],
        5 => &[Color, Cct],
        16 => &[Color, Segments],
        19 => &[Color, Wb, Motor, Laser],
        20 => &[Color, Wb, Segments],
        21 => &[Color, Cct, Segments],
        _ => &[Color, Cct, Wb]
    }
}

pub fn is_magic(group: u8) -> bool {
    [16, 20, 21].contains(&group)
}

pub fn is_two_path(group: u8) -> bool {
    [17, 18].contains(&group)
}

pub fn is_sphere(group: u8) -> bool {
    group == 19
}

// Indexes are stored in saved segments and favourites, so new colours are only appended.
// PALETTE_ORDER is the display order (rainbow, white last).
pub const CLASSIC_RGB: [[u8; 3]; 16] = [
    [255, 0, 0],
    [255, 0, 255],
    [0, 0, 255],
    [0, 255, 255],
    [0, 255, 0],
    [255, 255, 0],
    [255, 255, 255],
    [255, 128, 0],
    [255, 0, 128],
    [128, 0, 255],
    [0, 255, 128],
    [255, 64, 0],
    [255, 192, 0],
    [128, 255, 0],
    [0, 128, 255],
    [64, 0, 255]
];
pub const CLASSIC_NAMES: [&str; 16] = [
    "red",
    "magenta",
    "blue",
    "cyan",
    "green",
    "yellow",
    "white",
    "orange",
    "pink",
    "purple",
    "spring green",
    "red-orange",
    "amber",
    "lime",
    "azure",
    "indigo"
];
pub const PALETTE_ORDER: [usize; 16] = [0, 11, 7, 12, 5, 13, 4, 10, 3, 14, 2, 15, 9, 1, 8, 6];

pub fn palette_index_by_name(name: &str) -> Option<usize> {
    let wanted = name.trim().to_lowercase().replace(['-', '_'], " ");
    CLASSIC_NAMES.iter().position(|n| n.replace('-', " ") == wanted)
}

// Dot colours used to preview mode colour sets
pub fn dot(key: &str) -> ([u8; 3], &'static str) {
    match key {
        "RD" => ([0xE5, 0x48, 0x4D], "red"),
        "GN" => ([0x46, 0xA7, 0x58], "green"),
        "BU" => ([0x3E, 0x63, 0xDD], "blue"),
        "YE" => ([0xF5, 0xD9, 0x0A], "yellow"),
        "CYAN" => ([0x00, 0xC2, 0xD7], "cyan"),
        "VT" => ([0x8E, 0x4E, 0xC6], "violet"),
        "OR" | "OG" => ([0xF7, 0x6B, 0x15], "orange"),
        _ => ([0xFF, 0xFF, 0xFF], "white")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dot {
    pub rgb: [u8; 3],
    pub name: &'static str
}

fn dots_of(name: &str) -> Vec<Dot> {
    name.split('_')
        .map(|k| {
            let (rgb, name) = dot(k);
            Dot { rgb, name }
        })
        .collect()
}

// R.array.mode_colors (magic), mode_rgb_colors, mode_sphere_colors
const MAGIC_COLOR_NAMES: [&str; 19] = [
    "RD_GN", "RD_BU", "GN_BU", "RD_GN_BU", "BU_YE_CYAN", "CYAN_VT_OG", "RD_GN_BU_YE", "GN_BU_YE_CYAN", "BU_YE_CYAN_VT",
    "RD_GN_BU_YE_CYAN", "GN_BU_YE_CYAN_VT", "BU_YE_CYAN_VT_OR", "RD_GN_BU_YE_CYAN_VT", "GN_BU_YE_CYAN_VT_OR",
    "BU_YE_CYAN_VT_OR_WT", "RD_GN_BU_YE_CYAN_VT_OG", "GN_BU_YE_CYAN_VT_OR_WT", "RD_OR_YE_GN_CYAN_BU_VT",
    "RD_GN_BU_YE_CYAN_VT_OR_WT"
];
const RGB_COLOR_NAMES: [&str; 30] = [
    "OR_WT", "VT_OR", "CYAN_VT", "YE_CYAN", "BU_YE", "GN_BU", "RD_GN", "VT_OR_WT", "CYAN_VT_OR", "YE_CYAN_VT",
    "BU_YE_CYAN", "GN_BU_YE", "RD_GN_BU", "CYAN_VT_OR_WT", "YE_CYAN_VT_OR", "BU_YE_CYAN_VT", "GN_BU_YE_CYAN",
    "RD_GN_BU_YE", "YE_CYAN_VT_OR_WT", "BU_YE_CYAN_VT_OR", "GN_BU_YE_CYAN_VT", "RD_GN_BU_YE_CYAN",
    "BU_YE_CYAN_VT_OR_WT", "GN_BU_YE_CYAN_VT_OR", "RD_GN_BU_YE_CYAN_VT", "RD_OR_YE_GN_CYAN_BU_VT",
    "GN_BU_YE_CYAN_VT_OR_WT", "RD_GN_BU_YE_CYAN_VT_OG", "RD_OR_YE_GN_CYAN_BU_VT_WT", "RD_GN_BU_YE_CYAN_VT_OR_WT"
];
const SPHERE_COLOR_NAMES: [&str; 8] = ["RD_GN", "RD_BU", "RD_WT", "GN_BU", "GN_WT", "BU_WT", "RD_GN_BU", "RD_GN_BU_WT"];

// Mode.RGB_MODES / Mode.SPHERE_MODES: (colorId, colorValue, colorCount)
const RGB_MODES: [(usize, &str, u8); 30] = [
    (0, "67", 2),
    (1, "56", 2),
    (2, "45", 2),
    (3, "34", 2),
    (4, "23", 2),
    (5, "12", 2),
    (6, "01", 2),
    (7, "5670", 3),
    (8, "4560", 3),
    (9, "3450", 3),
    (10, "2340", 3),
    (11, "1230", 3),
    (12, "0120", 3),
    (13, "4567", 4),
    (14, "3456", 4),
    (15, "2345", 4),
    (16, "1234", 4),
    (17, "0123", 4),
    (18, "345670", 5),
    (19, "234560", 5),
    (20, "123450", 5),
    (21, "012340", 5),
    (22, "234567", 6),
    (23, "123456", 6),
    (24, "012345", 6),
    (25, "06314250", 7),
    (26, "12345670", 7),
    (27, "01234560", 7),
    (28, "06314257", 8),
    (29, "01234567", 8)
];
const SPHERE_MODES: [(usize, &str, u8); 8] = [
    (0, "01000000", 2),
    (1, "02000000", 2),
    (2, "07000000", 2),
    (3, "12000000", 2),
    (4, "17000000", 2),
    (5, "27000000", 2),
    (6, "01200000", 3),
    (7, "01270000", 4)
];
pub const RGB_SCENES: [&str; 4] = ["Fade", "Jump", "Breathe", "Flash"];

// Mode.MAGIC_MODES: 18 scenes x 19 colour sets (colorValue indexed by colorId)
const MAGIC_VALUES: [&str; 19] = [
    "0201000000", "0202000000", "0212000000", "0301200000", "0323400000", "0345600000", "0401230000", "0412340000",
    "0423450000", "0501234000", "0512345000", "0523456000", "0601234500", "0612345600", "0623456700", "0701234560",
    "0712345670", "0706314250", "0801234567"
];
pub const MAGIC_SCENES: [&str; 18] = [
    "Fade",
    "Jump",
    "Breathe",
    "Flash",
    "Meteor",
    "Stack",
    "Float",
    "Follow spot",
    "Wave",
    "Water",
    "Rainbow",
    "Blink",
    "Bounce",
    "Shuttle",
    "Twinkle",
    "On/off",
    "Curtain",
    "Alternate"
];

// Two-path scenes: new Mode(i + 200), no colour data
pub const TWO_PATH_SCENES: [&str; 17] = [
    "Auto",
    "Multi colour",
    "Async jump",
    "Async fade",
    "Jump",
    "Fade",
    "Twinkle 1",
    "Twinkle 2",
    "Strobe 1",
    "Strobe 2",
    "Flick",
    "Breathe",
    "WW flick",
    "WW breathe",
    "RGBW jump",
    "RGBW flick",
    "In waves"
];

pub const RGB_EFFECTS: [&str; 4] = ["Classic", "Soft", "Dynamic", "Disco"];
pub const MAGIC_EFFECTS: [&str; 4] = ["Classic", "Roll", "Energy", "Spectrum"];
pub const MIC_COLORS: [[u8; 3]; 6] = [[255, 0, 0], [255, 125, 0], [255, 255, 0], [0, 255, 0], [0, 0, 255], [0, 255, 255]];
pub const IC_ORDERS: [&str; 6] = ["RGB", "RBG", "GRB", "GBR", "BRG", "BGR"];
pub const WEEKDAYS: [(&str, u8); 7] = [
    ("Mon", 1),
    ("Tue", 2),
    ("Wed", 4),
    ("Thu", 8),
    ("Fri", 16),
    ("Sat", 32),
    ("Sun", 64)
];

pub fn mic_effects(group: u8) -> &'static [&'static str; 4] {
    if is_magic(group) {
        return &MAGIC_EFFECTS;
    }
    &RGB_EFFECTS
}

pub fn hex_bytes(s: &str) -> Vec<u8> {
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap_or(0)).collect()
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModeItem {
    pub dots: Vec<Dot>,
    pub label: String,
    pub head: Vec<u8>,
    pub tail: Vec<u8>
}

fn build_magic_scene(id: u8) -> Vec<ModeItem> {
    let mut items = Vec::new();
    for (color_id, value) in MAGIC_VALUES.iter().enumerate() {
        let variants: &[u8] = if id <= 4 || id <= 11 || id == 14 || id == 16 || id == 17 { &[0, 1] } else { &[0] };
        for &v in variants {
            // v is "section" for 1-4, "direction" for 5-11, "colourful" for 14, "alternate" for 16, "status" for 17
            let fmt = if id <= 4 { (id & 31) | (v << 7) } else { (id & 31) | (v << 6) };
            let suffix = if id < 5 {
                if v == 0 { "" } else { "by section" }
            } else if id < 12 {
                if v == 0 { "forward" } else { "reverse" }
            } else if id == 14 {
                if v == 0 { "colour" } else { "multicolour" }
            } else if id == 16 {
                if v == 0 { "" } else { "alternate" }
            } else if id == 17 {
                if v == 0 { "curtain up" } else { "curtain down" }
            } else {
                ""
            };
            items.push(ModeItem {
                dots: dots_of(MAGIC_COLOR_NAMES[color_id]),
                label: suffix.to_string(),
                head: vec![fmt],
                tail: hex_bytes(value)
            });
        }
    }
    items
}

pub fn scene_tabs(group: u8) -> &'static [&'static str] {
    if is_magic(group) {
        return &MAGIC_SCENES;
    }
    if is_two_path(group) {
        return &[];
    }
    &RGB_SCENES
}

pub fn mode_items(group: u8, scene_tab: usize) -> Vec<ModeItem> {
    if is_magic(group) {
        if scene_tab >= MAGIC_SCENES.len() {
            return Vec::new();
        }
        return build_magic_scene(scene_tab as u8 + 1);
    }
    if is_two_path(group) {
        return TWO_PATH_SCENES
            .iter()
            .enumerate()
            .map(|(i, name)| ModeItem {
                dots: Vec::new(),
                label: name.to_string(),
                head: vec![((200 + i) & 31) as u8],
                tail: Vec::new()
            })
            .collect();
    }
    let (list, names): (&[(usize, &str, u8)], &[&str]) =
        if is_sphere(group) { (&SPHERE_MODES, &SPHERE_COLOR_NAMES) } else { (&RGB_MODES, &RGB_COLOR_NAMES) };
    list.iter()
        .map(|(color_id, value, count)| {
            let mut tail = vec![*count];
            tail.extend(hex_bytes(value));
            ModeItem { dots: dots_of(names[*color_id]), label: String::new(), head: vec![scene_tab as u8 + 1], tail }
        })
        .collect()
}

// RGB/sphere: 07 scene speed brightness count colours; magic/2-path: 07 mode speed brightness colours
pub fn mode_packet(item: &ModeItem, speed: u8, brightness: u8) -> Vec<u8> {
    let mut p = vec![0x07];
    p.extend(&item.head);
    p.push(speed);
    p.push(brightness);
    p.extend(&item.tail);
    p
}

pub fn power_packet(on: bool) -> Vec<u8> {
    vec![0x01, on as u8]
}

pub fn brightness_packet(b: u8) -> Vec<u8> {
    vec![0x02, b]
}

pub fn rgb_packet(rgb: [u8; 3]) -> Vec<u8> {
    vec![0x03, rgb[0], rgb[1], rgb[2], 0, 0, 0]
}

// c: 0 warm, 255 cold
pub fn cct_packet(c: u8) -> Vec<u8> {
    vec![0x03, 0, 0, 0, 0, c, 1]
}

pub fn white_packet(w: u8) -> Vec<u8> {
    vec![0x03, 0, 0, 0, w, 0, 1]
}

pub fn device_mic_packet(effect: u8, sensitivity: u8) -> Vec<u8> {
    vec![0x04, effect, sensitivity]
}

// The original app collects "turn on / turn off" but never sends it, the third byte is always 01.
pub fn delay_packet(enabled: bool, minutes: u16) -> Vec<u8> {
    vec![0x05, enabled as u8, 0x01, (minutes & 0xff) as u8, (minutes >> 8) as u8]
}

pub fn time_packet(now: chrono::DateTime<chrono::Local>) -> Vec<u8> {
    use chrono::{Datelike, Timelike};
    let y = now.year() as u16;
    vec![
        0x08,
        0x01,
        0x00,
        0x00,
        0x00,
        (y & 0xff) as u8,
        (y >> 8) as u8,
        now.month() as u8,
        now.day() as u8,
        now.hour() as u8,
        now.minute() as u8,
        now.second() as u8,
    ]
}

pub fn ic_order_packet(order_index: usize) -> Vec<u8> {
    vec![0x0e, order_index as u8 + 1]
}

pub fn ic_length_packet(n: u16) -> Vec<u8> {
    vec![0x0f, (n & 0xff) as u8, (n >> 8) as u8]
}

pub fn phone_frame_packet(rgb: [u8; 3], level: u8, effect: u8) -> Vec<u8> {
    vec![0x10, rgb[0], rgb[1], rgb[2], 0, 0, level, effect]
}

pub fn phone_start_packet() -> Vec<u8> {
    vec![0x11, 0x04]
}

pub fn motor_packet(on: bool, speed: u8) -> Vec<u8> {
    vec![0x15, on as u8, 0x00, speed]
}

pub fn laser_packet(on: bool, speed: u8) -> Vec<u8> {
    vec![0x17, on as u8, 0x00, speed, 0x64]
}

// Lighting commands that must drive the whole strip again after segments shrank the IC length.
pub const LIGHTING_OPCODES: [u8; 8] = [0x01, 0x03, 0x04, 0x07, 0x10, 0x11, 0x15, 0x17];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Alarm {
    pub hour: u8,
    pub minute: u8,
    pub weeks: u8,
    pub on: bool,
    pub enable: bool
}

// Up to 6 enabled alarms, 3 bytes each: hour | index << 5, minute | 0x80 if turn on, weekdays.
pub fn alarms_packet(alarms: &[Alarm]) -> Vec<u8> {
    let active: Vec<&Alarm> = alarms.iter().filter(|a| a.enable).take(6).collect();
    let mut bytes = vec![0u8; if active.is_empty() { 3 } else { active.len() * 3 + 1 }];
    bytes[0] = 0x0d;
    for (i, a) in active.iter().enumerate() {
        bytes[i * 3 + 1] = a.hour | ((i as u8) << 5);
        bytes[i * 3 + 2] = a.minute | if a.on { 0x80 } else { 0 };
        bytes[i * 3 + 3] = a.weeks & 0x7f;
    }
    bytes
}

pub fn weeks_text(w: u8) -> String {
    match w {
        0 => "no repeat".to_string(),
        127 => "every day".to_string(),
        31 => "weekdays".to_string(),
        96 => "weekend".to_string(),
        _ => WEEKDAYS.iter().filter(|(_, bit)| w & bit != 0).map(|(n, _)| *n).collect::<Vec<_>>().join(" ")
    }
}

pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> [u8; 3] {
    let c = v * s;
    let hp = (h % 360.0) / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r, g, b) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let m = v - c;
    [js_round((r + m) * 255.0) as u8, js_round((g + m) * 255.0) as u8, js_round((b + m) * 255.0) as u8]
}

pub fn rgb_to_hs(rgb: [u8; 3]) -> (f64, f64) {
    let (r, g, b) = (rgb[0] as f64, rgb[1] as f64, rgb[2] as f64);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = 0.0;
    if d != 0.0 {
        h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
    }
    if h < 0.0 {
        h += 360.0;
    }
    (h, if max == 0.0 { 0.0 } else { d / max })
}

// JavaScript Math.round: halves round towards +infinity.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

pub fn cct_presets() -> Vec<u8> {
    [0.0, 0.167, 0.334, 0.501, 0.668, 0.835, 1.0].iter().map(|s| js_round(255.0 * (1.0 - s)) as u8).collect()
}

pub fn wb_presets() -> Vec<u8> {
    [1.0, 0.835, 0.668, 0.501, 0.334, 0.167, 0.0].iter().map(|v| js_round(255.0 * v) as u8).collect()
}

// SystemRecorder: 20*log10(mean |sample|) * 2.55, clamped to 0-255.
// Samples are floats in -1..1 scaled to 16-bit integers like the analyser data in the browser.
pub fn mic_volume(samples: &[f32]) -> u8 {
    if samples.is_empty() {
        return 0;
    }
    let sum: f64 = samples.iter().map(|s| js_round(*s as f64 * 32767.0).abs()).sum();
    let mean = sum / samples.len() as f64;
    let vol = if mean > 0.0 { (20.0 * mean.log10() * 2.55).floor() } else { 0.0 };
    vol.clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_scene_has_both_variants_and_encodes_mode_byte() {
        let fade = mode_items(16, 0);
        assert_eq!(fade.len(), 38);
        assert_eq!(fade[0].head, vec![1]);
        assert_eq!(fade[1].head, vec![1 | 0x80]);
        assert_eq!(fade[1].label, "by section");
        assert_eq!(fade[0].tail, vec![0x02, 0x01, 0x00, 0x00, 0x00]);

        let meteor = mode_items(16, 4);
        assert_eq!(meteor[1].head, vec![5 | 0x40]);
        assert_eq!(meteor[1].label, "reverse");

        let shuttle = mode_items(16, 13);
        assert_eq!(shuttle[1].head, vec![14 | 0x40]);
        assert_eq!(shuttle[1].label, "multicolour");

        let twinkle = mode_items(16, 14);
        assert_eq!(twinkle.len(), 19);
        assert_eq!(twinkle[0].head, vec![15]);

        let curtain = mode_items(16, 16);
        assert_eq!(curtain[1].head, vec![17 | 0x40]);
        assert_eq!(curtain[1].label, "curtain down");

        let alternate = mode_items(16, 17);
        assert_eq!(alternate.len(), 19);
    }

    #[test]
    fn rgb_and_two_path_modes() {
        let items = mode_items(3, 2);
        assert_eq!(items.len(), 30);
        assert_eq!(mode_packet(&items[0], 10, 100), vec![0x07, 3, 10, 100, 2, 0x67]);
        let sphere = mode_items(19, 0);
        assert_eq!(sphere.len(), 8);
        assert_eq!(sphere[7].tail, vec![4, 0x01, 0x27, 0x00, 0x00]);
        let two = mode_items(17, 0);
        assert_eq!(two.len(), 17);
        assert_eq!(two[0].head, vec![200 & 31]);
        assert_eq!(mode_packet(&two[16], 0, 100), vec![0x07, 216 & 31, 0, 100]);
    }

    #[test]
    fn alarms_encoding() {
        assert_eq!(alarms_packet(&[]), vec![0x0d, 0, 0]);
        let a = Alarm { hour: 7, minute: 30, weeks: 31, on: true, enable: true };
        let b = Alarm { hour: 23, minute: 5, weeks: 0, on: false, enable: true };
        let off = Alarm { hour: 1, minute: 1, weeks: 1, on: true, enable: false };
        assert_eq!(alarms_packet(&[a, off, b]), vec![0x0d, 7, 30 | 0x80, 31, 23 | (1 << 5), 5, 0]);
    }

    #[test]
    fn presets_match_browser() {
        assert_eq!(cct_presets(), vec![255, 212, 170, 127, 85, 42, 0]);
        assert_eq!(wb_presets(), vec![255, 213, 170, 128, 85, 43, 0]);
    }

    #[test]
    fn colour_conversions() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv_to_rgb(0.0, 0.0, 1.0), [255, 255, 255]);
        let (h, s) = rgb_to_hs([255, 0, 255]);
        assert_eq!((h, s), (300.0, 1.0));
    }

    #[test]
    fn packets() {
        assert_eq!(delay_packet(true, 90), vec![0x05, 1, 1, 90, 0]);
        assert_eq!(ic_length_packet(300), vec![0x0f, 0x2c, 0x01]);
        assert_eq!(laser_packet(true, 50), vec![0x17, 1, 0, 50, 0x64]);
        assert_eq!(weeks_text(5), "Mon Wed");
        assert_eq!(mic_volume(&[0.0; 4]), 0);
        assert_eq!(mic_volume(&[1.0; 4]), 230);
    }
}
