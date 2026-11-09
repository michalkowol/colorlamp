use crate::ble::{Advertisement, Ble, BleEvent};
use crate::mic::MicCapture;
use crate::protocol::*;
use crate::segments;
use crate::store::{Delay, Favorite, FavoriteKind, KnownDevice, Segment, Setup, SetupLamp, Store, new_id};
use futures::FutureExt;
use futures::future::join_all;
use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::task::AbortHandle;

const LOG_SIZE: usize = 40;
const ADVERTISEMENT_WINDOW: Duration = Duration::from_secs(10);
const AUTO_CONNECT_ATTEMPTS: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainTab {
    Devices,
    Color,
    Mode,
    Rhythm,
    Timer,
    Settings,
    Favorites,
    Setups
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicSource {
    Device,
    Phone
}

#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub ble_name: String,
    pub connected: bool,
    pub connecting: bool,
    pub waiting: bool,
    pub manual: bool,
    pub selected: bool,
    pub group: Option<u8>,
    pub kind: Option<u8>,
    pub shortened: bool,
    pub last_favorite: String,
    pub rssi: Option<i16>,
    // waiting for the next advertisement to retry the connection
    watching: bool,
    watch_attempt: u32,
    // reads the [group, type] manufacturer data for a while after connecting
    advertisement_until: Option<Instant>
}

impl Device {
    pub fn status(&self) -> &'static str {
        if self.connected {
            "connected"
        } else if self.connecting {
            "connecting…"
        } else if self.waiting {
            "waiting for the lamp, will connect automatically…"
        } else {
            "disconnected"
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub stamp: String,
    pub text: String,
    pub dim: bool
}

#[derive(Clone, Debug)]
pub struct AlarmForm {
    pub index: Option<usize>,
    pub time: String,
    pub weeks: u8,
    pub on: bool,
    pub enable: bool
}

#[derive(Clone, Debug)]
pub struct DelayForm {
    pub hour: i64,
    pub minute: i64,
    pub enable: bool
}

#[derive(Clone, Debug)]
pub struct SetupForm {
    pub name: String,
    // device id -> "" (leave unchanged), "off" or a favourite id
    pub lamps: Vec<(String, String)>
}

pub struct State {
    pub store: Store,
    path: PathBuf,
    pub devices: Vec<Device>,
    pub discovered: Vec<Advertisement>,
    pub choosing: bool,
    pub ble_error: Option<String>,
    pub tab: MainTab,
    pub power: bool,
    pub toast: Option<(String, Instant)>,
    pub echo_toast: bool,
    pub echo_log: bool,
    // CLI: wait for the lamp to acknowledge every write, since it disconnects right after sending.
    pub confirm_writes: bool,
    pub log: VecDeque<LogEntry>,

    pub color_tab: ColorTab,
    pub rgb: [u8; 3],
    pub hue: f64,
    pub sat: f64,
    pub preset_index: Option<usize>,
    pub cct: u8,
    pub wb: u8,
    pub motor_on: bool,
    pub motor_speed: u8,
    pub laser_on: bool,
    pub laser_speed: u8,

    pub scene_tab: usize,
    pub mode_index: Option<usize>,
    pub speed: u8,
    pub mode_brightness: u8,

    pub mic_source: MicSource,
    pub sensitivity: u8,
    pub mic_effect: u8,
    pub recording: bool,
    pub level: u8,
    pub mic_color_index: usize,

    pub setup_form: Option<SetupForm>,
    pub alarm_form: Option<AlarmForm>,
    pub delay_form: Option<DelayForm>,
    pub segments_busy: bool,

    pending: HashMap<&'static str, Vec<u8>>,
    flushing: HashSet<&'static str>,
    retries: HashMap<String, AbortHandle>,
    last_group: u8,
    mic_generation: u64
}

impl State {
    pub fn new(store: Store, path: PathBuf) -> State {
        State {
            store,
            path,
            devices: Vec::new(),
            discovered: Vec::new(),
            choosing: false,
            ble_error: None,
            tab: MainTab::Color,
            power: false,
            toast: None,
            echo_toast: false,
            echo_log: false,
            confirm_writes: false,
            log: VecDeque::new(),
            color_tab: ColorTab::Color,
            rgb: [255, 0, 0],
            hue: 0.0,
            sat: 1.0,
            preset_index: Some(0),
            cct: 255,
            wb: 255,
            motor_on: false,
            motor_speed: 50,
            laser_on: false,
            laser_speed: 50,
            scene_tab: 0,
            mode_index: None,
            speed: 0,
            mode_brightness: 100,
            mic_source: MicSource::Device,
            sensitivity: 50,
            mic_effect: 0,
            recording: false,
            level: 0,
            mic_color_index: 0,
            setup_form: None,
            alarm_form: None,
            delay_form: None,
            segments_busy: false,
            pending: HashMap::new(),
            flushing: HashSet::new(),
            retries: HashMap::new(),
            last_group: DEFAULT_GROUP,
            mic_generation: 0
        }
    }

    // ---------- persistence ----------
    pub fn persist(&self) {
        // storage unavailable: keep going with the in-memory state
        let _ = self.store.save(&self.path);
    }

    pub fn state_path(&self) -> &PathBuf {
        &self.path
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast_for(text, 3200);
    }

    pub fn toast_for(&mut self, text: impl Into<String>, ms: u64) {
        let text = text.into();
        if self.echo_toast {
            eprintln!("{text}");
        }
        self.toast = Some((text, Instant::now() + Duration::from_millis(ms)));
    }

    pub fn toast_text(&self) -> Option<&str> {
        match &self.toast {
            Some((text, until)) if *until > Instant::now() => Some(text),
            _ => None
        }
    }

    pub fn push_log(&mut self, text: String, dim: bool) {
        if self.echo_log {
            println!("{}{text}", if dim { "(not sent) " } else { "" });
        }
        let stamp = chrono::Local::now().format("%H:%M:%S").to_string();
        self.log.push_front(LogEntry { stamp, text, dim });
        self.log.truncate(LOG_SIZE);
    }

    // ---------- devices ----------
    pub fn connected_count(&self) -> usize {
        self.devices.iter().filter(|d| d.connected).count()
    }

    pub fn selected_devices(&self) -> Vec<&Device> {
        self.devices.iter().filter(|d| d.selected).collect()
    }

    pub fn target_devices(&self) -> Vec<String> {
        self.devices.iter().filter(|d| d.selected && d.connected).map(|d| d.id.clone()).collect()
    }

    pub fn group(&self) -> u8 {
        self.selected_devices().first().and_then(|d| d.group).unwrap_or(DEFAULT_GROUP)
    }

    pub fn group_known(&self) -> bool {
        self.selected_devices().first().map(|d| d.group.is_some()).unwrap_or(false)
    }

    pub fn is_magic(&self) -> bool {
        is_magic(self.group())
    }

    pub fn is_two_path(&self) -> bool {
        is_two_path(self.group())
    }

    pub fn color_tabs(&self) -> &'static [ColorTab] {
        color_tabs(self.group())
    }

    // Lamp type changed: reset the colour tab and the mode selection.
    pub fn sync_group(&mut self) {
        let g = self.group();
        if g != self.last_group {
            self.last_group = g;
            self.color_tab = color_tabs(g)[0];
            self.scene_tab = 0;
            self.mode_index = None;
        }
    }

    pub fn dev(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn dev_mut(&mut self, id: &str) -> Option<&mut Device> {
        self.devices.iter_mut().find(|d| d.id == id)
    }

    fn known_mut(&mut self, id: &str) -> &mut KnownDevice {
        if self.store.device(id).is_none() {
            let ble_name = self.dev(id).map(|d| d.ble_name.clone()).unwrap_or_else(|| "Lamp".to_string());
            self.store.devices.push(KnownDevice { id: id.to_string(), ble_name, name: String::new(), group: None, shortened: false });
        }
        self.store.device_mut(id).unwrap()
    }

    pub fn register_device(&mut self, id: &str, name: Option<String>) {
        if self.dev(id).is_some() {
            return;
        }
        let known = self.store.device(id).cloned();
        let ble_name = name
            .filter(|n| !n.is_empty())
            .or_else(|| known.as_ref().map(|k| k.ble_name.clone()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Lamp".to_string());
        let custom = known.as_ref().map(|k| k.name.clone()).unwrap_or_default();
        // Every lamp is controlled right away, as long as it has the type of the lamps already selected.
        let group = Some(known.as_ref().and_then(|k| k.group).unwrap_or(DEFAULT_GROUP));
        let selected = self.selected_devices().first().map(|f| f.group == group).unwrap_or(true);
        self.devices.push(Device {
            id: id.to_string(),
            name: if custom.is_empty() { ble_name.clone() } else { custom },
            ble_name: ble_name.clone(),
            connected: false,
            connecting: false,
            waiting: false,
            manual: false,
            selected,
            group,
            kind: None,
            shortened: known.as_ref().map(|k| k.shortened).unwrap_or(false),
            last_favorite: String::new(),
            rssi: None,
            watching: false,
            watch_attempt: 0,
            advertisement_until: None
        });
        self.known_mut(id).ble_name = ble_name;
        self.persist();
        self.sync_group();
    }

    pub fn toggle_select(&mut self, id: &str) {
        let Some(d) = self.dev(id) else { return };
        if d.selected {
            self.dev_mut(id).unwrap().selected = false;
            self.sync_group();
            return;
        }
        let first_group = self.selected_devices().first().and_then(|f| f.group);
        if let (Some(fg), Some(dg)) = (first_group, d.group)
            && fg != dg
        {
            self.toast("Only lamps of the same type can be controlled together.");
            return;
        }
        self.dev_mut(id).unwrap().selected = true;
        self.sync_group();
    }

    pub fn set_group(&mut self, id: &str, group: Option<u8>) {
        let Some(d) = self.dev_mut(id) else { return };
        d.group = group;
        self.known_mut(id).group = group;
        self.persist();
        self.sync_group();
    }

    pub fn rename_device(&mut self, id: &str, name: &str) {
        let Some(d) = self.dev_mut(id) else { return };
        let trimmed = name.trim();
        d.name = if trimmed.is_empty() { d.ble_name.clone() } else { trimmed.to_string() };
        let custom = if d.name == d.ble_name { String::new() } else { d.name.clone() };
        self.known_mut(id).name = custom;
        self.persist();
    }

    fn apply_manufacturer(&mut self, id: &str, data: &[u8]) {
        if data.len() < 2 {
            return;
        }
        let Some(d) = self.dev_mut(id) else { return };
        d.group = Some(data[0]);
        d.kind = Some(data[1]);
        d.advertisement_until = None;
        self.known_mut(id).group = Some(data[0]);
        // The advertised type may differ from the stored one: keep only lamps of one type selected.
        let other = self.devices.iter().find(|o| o.selected && o.id != id).map(|o| o.group);
        if let Some(g) = other
            && g != Some(data[0])
            && let Some(d) = self.dev_mut(id)
        {
            d.selected = false;
        }
        self.persist();
        self.sync_group();
    }

    fn set_shortened(&mut self, ids: &[String], v: bool) {
        for id in ids {
            if let Some(d) = self.dev_mut(id) {
                d.shortened = v;
                self.known_mut(id).shortened = v;
            }
        }
        self.persist();
    }

    pub fn segments_active(&self) -> bool {
        self.selected_devices().iter().any(|d| d.shortened)
    }

    fn abort_retry(&mut self, id: &str) {
        if let Some(h) = self.retries.remove(id) {
            h.abort();
        }
    }

    fn needs_scan(&self) -> bool {
        self.choosing || self.devices.iter().any(|d| !d.manual && !d.connected && (d.waiting || d.watching))
    }

    // ---------- colour ----------
    pub fn can_save_color(&self) -> bool {
        [ColorTab::Color, ColorTab::Cct, ColorTab::Wb].contains(&self.color_tab) && self.color_tabs().contains(&self.color_tab)
    }

    pub fn color_packet(&self) -> Vec<u8> {
        match self.color_tab {
            ColorTab::Cct => cct_packet(self.cct),
            ColorTab::Wb => white_packet(self.wb),
            _ => rgb_packet(self.rgb)
        }
    }

    pub fn color_swatch(&self) -> [u8; 3] {
        match self.color_tab {
            ColorTab::Cct => [255, 255, self.cct],
            ColorTab::Wb => [self.wb, self.wb, self.wb],
            _ => self.rgb
        }
    }

    // ---------- modes ----------
    pub fn scene_tabs(&self) -> &'static [&'static str] {
        scene_tabs(self.group())
    }

    pub fn mode_items(&self) -> Vec<ModeItem> {
        mode_items(self.group(), self.scene_tab)
    }

    pub fn mic_effects(&self) -> &'static [&'static str; 4] {
        mic_effects(self.group())
    }

    // Phone microphone frames carry their own RGB, so the rhythm can use one fixed colour
    // (micColor = palette index) or rotate like the original app (micColor = -1).
    pub fn current_mic_color(&self) -> [u8; 3] {
        let c = self.store.settings.mic_color;
        if c >= 0 { CLASSIC_RGB[c as usize] } else { MIC_COLORS[self.mic_color_index] }
    }

    // ---------- segments ----------
    // Shows a favourite on screen. Colour and mode only when the lamp type matches.
    fn restore_favorite_ui(&mut self, f: &Favorite) {
        self.power = true;
        match &f.kind {
            FavoriteKind::Segments { ic_length, segments, .. } => {
                self.tab = MainTab::Color;
                if self.color_tabs().contains(&ColorTab::Segments) {
                    self.color_tab = ColorTab::Segments;
                }
                self.store.settings.ic_length = *ic_length;
                self.store.segments = segments.iter().map(|s| Segment { color: s.color, end: s.end }).collect();
                self.persist();
            }
            _ if f.group != self.group() => {}
            FavoriteKind::Mode { scene_tab, mode_index, speed, .. } => {
                self.tab = MainTab::Mode;
                self.scene_tab = *scene_tab;
                self.mode_index = Some(*mode_index);
                self.speed = *speed;
                self.mode_brightness = f.brightness;
            }
            FavoriteKind::Color { color_tab, rgb, hue, sat, cct, wb, .. } => {
                self.tab = MainTab::Color;
                self.color_tab = *color_tab;
                self.rgb = *rgb;
                self.hue = *hue;
                self.sat = *sat;
                self.cct = *cct;
                self.wb = *wb;
                self.store.settings.brightness = f.brightness;
                self.preset_index = None;
                self.persist();
            }
        }
    }

    pub fn segments_error(&self) -> Option<String> {
        segments::segments_error(&self.store.segments, self.store.settings.ic_length)
    }

    pub fn segment_steps(&self) -> Vec<segments::Step> {
        segments::segment_steps(&self.store.segments, self.store.settings.ic_length)
    }

    pub fn segment_percent(&self, i: usize) -> String {
        segments::segment_percent(&self.store.segments, self.store.settings.ic_length, i)
    }

    pub fn split_segments_evenly(&mut self) {
        let full = self.store.settings.ic_length;
        segments::split_evenly(&mut self.store.segments, full);
        self.persist();
    }

    pub fn add_segment(&mut self) {
        if let Err(e) = segments::add_segment(&mut self.store.segments) {
            self.toast(e);
            return;
        }
        self.persist();
    }

    pub fn remove_segment(&mut self, i: usize) {
        segments::remove_segment(&mut self.store.segments, i);
        self.persist();
    }

    // ---------- favourites ----------
    pub fn favorite_details(&self, f: &Favorite) -> String {
        let mut parts = Vec::new();
        match &f.kind {
            FavoriteKind::Mode { speed, .. } => parts.push(format!("speed {speed}")),
            FavoriteKind::Segments { segments, ic_length, .. } => {
                parts.push(format!("{} segments · {ic_length} LEDs", segments.len()));
                if f.group != self.group() {
                    parts.push(group_name(f.group));
                }
                return parts.join(" · ");
            }
            FavoriteKind::Color { .. } => {}
        }
        parts.push(format!("brightness {}%", f.brightness));
        if f.group != self.group() {
            parts.push(group_name(f.group));
        }
        parts.join(" · ")
    }

    fn push_favorite(&mut self, mut fav: Favorite) {
        fav.id = new_id();
        let name = fav.name.clone();
        self.store.favorites.push(fav);
        self.persist();
        self.toast(format!("Added to favourites: {name}"));
    }

    pub fn add_color_favorite(&mut self) {
        if !self.can_save_color() {
            return;
        }
        let name = match self.color_tab {
            ColorTab::Cct => format!("White temperature {}", self.cct),
            ColorTab::Wb => format!("White {}", self.wb),
            _ => format!("Colour {}, {}, {}", self.rgb[0], self.rgb[1], self.rgb[2])
        };
        let brightness = self.store.settings.brightness;
        self.push_favorite(Favorite {
            id: String::new(),
            name,
            group: self.group(),
            brightness,
            kind: FavoriteKind::Color {
                color_tab: self.color_tab,
                rgb: self.rgb,
                hue: self.hue,
                sat: self.sat,
                cct: self.cct,
                wb: self.wb,
                swatch: self.color_swatch(),
                packets: vec![self.color_packet(), brightness_packet(brightness)]
            }
        });
    }

    pub fn add_mode_favorite(&mut self) {
        let items = self.mode_items();
        let Some(m) = self.mode_index.and_then(|i| items.get(i)) else { return };
        let scene = if self.is_magic() {
            MAGIC_SCENES[self.scene_tab].to_string()
        } else if self.is_two_path() {
            m.label.clone()
        } else {
            RGB_SCENES[self.scene_tab].to_string()
        };
        let suffix = if !self.is_two_path() && !m.label.is_empty() { format!(" {}", m.label) } else { String::new() };
        self.push_favorite(Favorite {
            id: String::new(),
            name: scene + &suffix,
            group: self.group(),
            brightness: self.mode_brightness,
            kind: FavoriteKind::Mode {
                scene_tab: self.scene_tab,
                mode_index: self.mode_index.unwrap_or(0),
                speed: self.speed,
                dots: m.dots.iter().map(|d| d.rgb).collect(),
                packets: vec![mode_packet(m, self.speed, self.mode_brightness)]
            }
        });
    }

    pub fn add_segments_favorite(&mut self) {
        if let Some(err) = self.segments_error() {
            self.toast(err);
            return;
        }
        let segs = self.store.segments.clone();
        self.push_favorite(Favorite {
            id: String::new(),
            name: segments::segments_name(&segs),
            group: self.group(),
            brightness: self.store.settings.brightness,
            kind: FavoriteKind::Segments {
                ic_length: self.store.settings.ic_length,
                dots: segs.iter().rev().map(|s| CLASSIC_RGB[s.color.min(15)]).collect(),
                segments: segs
            }
        });
    }

    pub fn rename_favorite(&mut self, id: &str, name: &str) {
        if let Some(f) = self.store.favorites.iter_mut().find(|f| f.id == id) {
            f.name = name.to_string();
            self.persist();
        }
    }

    pub fn remove_favorite(&mut self, id: &str) {
        self.store.favorites.retain(|f| f.id != id);
        for s in &mut self.store.setups {
            s.lamps.retain(|l| l.action != id);
        }
        self.persist();
    }

    // ---------- setups: one favourite (or off) per lamp, applied together ----------
    pub fn new_setup(&mut self) {
        let lamps = self.devices.iter().map(|d| (d.id.clone(), d.last_favorite.clone())).collect();
        self.setup_form = Some(SetupForm { name: format!("Setup {}", self.store.setups.len() + 1), lamps });
    }

    pub fn save_setup(&mut self) {
        let Some(form) = self.setup_form.clone() else { return };
        let lamps: Vec<SetupLamp> = form
            .lamps
            .iter()
            .filter(|(_, action)| !action.is_empty())
            .map(|(device, action)| SetupLamp { device: device.clone(), action: action.clone() })
            .collect();
        if lamps.is_empty() {
            self.toast("Pick a favourite for at least one lamp.");
            return;
        }
        let name = if form.name.trim().is_empty() { "Setup".to_string() } else { form.name.trim().to_string() };
        self.store.setups.push(Setup { id: new_id(), name, lamps });
        self.persist();
        self.setup_form = None;
    }

    pub fn rename_setup(&mut self, id: &str, name: &str) {
        if let Some(s) = self.store.setups.iter_mut().find(|s| s.id == id) {
            s.name = name.to_string();
            self.persist();
        }
    }

    pub fn remove_setup(&mut self, id: &str) {
        self.store.setups.retain(|s| s.id != id);
        self.persist();
    }

    pub fn setup_details(&self, s: &Setup) -> String {
        s.lamps
            .iter()
            .map(|l| {
                let name = self.dev(&l.device).map(|d| d.name.clone()).unwrap_or_else(|| "unknown lamp".to_string());
                let action = if l.action == "off" {
                    "off".to_string()
                } else {
                    self.store.favorites.iter().find(|f| f.id == l.action).map(|f| f.name.clone()).unwrap_or_else(|| "?".into())
                };
                format!("{name}: {action}")
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    // ---------- timers ----------
    pub fn new_alarm(&mut self) {
        if self.store.alarms.len() >= 6 {
            return;
        }
        let now = chrono::Local::now().format("%H:%M").to_string();
        self.alarm_form = Some(AlarmForm { index: None, time: now, weeks: 0, on: true, enable: true });
    }

    pub fn edit_alarm(&mut self, i: usize) {
        let Some(a) = self.store.alarms.get(i) else { return };
        self.alarm_form = Some(AlarmForm {
            index: Some(i),
            time: format!("{:02}:{:02}", a.hour, a.minute),
            weeks: a.weeks,
            on: a.on,
            enable: a.enable
        });
    }

    pub fn open_delay(&mut self) {
        let d = self.store.delay.clone();
        self.delay_form = Some(DelayForm {
            hour: d.as_ref().map(|d| d.hour as i64).unwrap_or(0),
            minute: d.as_ref().map(|d| d.minute as i64).unwrap_or(30),
            enable: d.map(|d| d.enable).unwrap_or(true)
        });
    }
}

pub fn parse_time(text: &str) -> Option<(u8, u8)> {
    let (h, rest) = text.trim().split_once(':')?;
    if h.is_empty() || h.len() > 2 || !h.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let m: String = rest.chars().take(2).collect();
    if m.len() != 2 || !m.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((h.parse::<u8>().ok()?.min(23), m.parse::<u8>().ok()?.min(59)))
}

pub struct Core {
    st: Mutex<State>,
    pub ble: Ble,
    mic: Mutex<Option<MicCapture>>
}

pub type Shared = Arc<Core>;

impl Core {
    pub fn new(state: State, ble: Ble) -> Shared {
        Arc::new(Core { st: Mutex::new(state), ble, mic: Mutex::new(None) })
    }

    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub async fn start(self: &Arc<Self>) {
        match self.ble.events().await {
            Ok(mut rx) => {
                let me = self.clone();
                tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        me.handle_event(event);
                    }
                });
            }
            Err(e) => {
                let mut st = self.lock();
                if st.ble_error.is_none() {
                    st.ble_error = Some(format!("Bluetooth is not available: {e}"));
                }
            }
        }
    }

    fn handle_event(self: &Arc<Self>, event: BleEvent) {
        match event {
            BleEvent::Advertisement(adv) => {
                let mut st = self.lock();
                if adv.is_lamp {
                    match st.discovered.iter_mut().find(|a| a.id == adv.id) {
                        Some(a) => *a = adv.clone(),
                        None => st.discovered.push(adv.clone())
                    }
                }
                let Some(d) = st.dev_mut(&adv.id) else { return };
                d.rssi = adv.rssi.or(d.rssi);
                let read = d.advertisement_until.map(|t| t > Instant::now()).unwrap_or(false);
                let retry = d.watching && !d.connected && !d.connecting && !d.manual;
                let attempt = d.watch_attempt;
                if retry {
                    d.watching = false;
                }
                if read && let Some(data) = &adv.manufacturer {
                    st.apply_manufacturer(&adv.id, data);
                }
                drop(st);
                if retry {
                    self.auto_connect(adv.id.clone(), attempt);
                }
            }
            BleEvent::Disconnected(id) => self.on_disconnected(&id)
        }
    }

    fn update_scan(self: &Arc<Self>) {
        let need = self.lock().needs_scan();
        if !self.ble.available() {
            return;
        }
        let ble = self.ble.clone();
        tokio::spawn(async move {
            let _ = ble.set_scanning(need).await;
        });
    }

    pub fn set_choosing(self: &Arc<Self>, on: bool) {
        {
            let mut st = self.lock();
            st.choosing = on;
            if on {
                st.discovered.clear();
            }
        }
        self.update_scan();
    }

    // Lamps added earlier connect without the chooser; a lamp that is off is retried when it advertises.
    pub fn restore_devices(self: &Arc<Self>) {
        let known: Vec<KnownDevice> = self.lock().store.devices.clone();
        for k in known {
            self.lock().register_device(&k.id, Some(k.ble_name.clone()));
            self.auto_connect(k.id.clone(), 0);
        }
    }

    pub async fn add_device(self: &Arc<Self>, id: &str, name: Option<String>) {
        {
            let mut st = self.lock();
            st.register_device(id, name);
            if let Some(d) = st.dev_mut(id) {
                d.manual = false;
            }
        }
        self.connect(id, false).await;
        self.read_advertisement(id).await;
    }

    pub fn auto_connect(self: &Arc<Self>, id: String, attempt: u32) {
        let me = self.clone();
        tokio::spawn(async move { me.auto_connect_run(id, attempt).await });
    }

    async fn auto_connect_run(self: &Arc<Self>, id: String, attempt: u32) {
        {
            let mut st = self.lock();
            let Some(d) = st.dev_mut(&id) else { return };
            if d.manual || d.connected || d.connecting {
                return;
            }
            d.waiting = true;
            st.abort_retry(&id);
        }
        self.update_scan();
        if self.connect(&id, true).await {
            if let Some(d) = self.lock().dev_mut(&id) {
                d.waiting = false;
            }
            self.read_advertisement(&id).await;
            self.update_scan();
            return;
        }
        {
            let mut st = self.lock();
            let Some(d) = st.dev_mut(&id) else { return };
            if d.manual {
                d.waiting = false;
                return;
            }
            // Out of range or asleep: connect as soon as the lamp advertises again, with a timer as fallback.
            d.watching = true;
            d.watch_attempt = attempt;
            if attempt < AUTO_CONNECT_ATTEMPTS {
                let me = self.clone();
                let retry_id = id.clone();
                let delay = Duration::from_millis(30000.min(3000 * (attempt as u64 + 1)));
                let handle = tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    me.auto_connect(retry_id, attempt + 1);
                });
                st.retries.insert(id.clone(), handle.abort_handle());
            } else {
                d.waiting = false;
            }
        }
        self.update_scan();
    }

    pub async fn connect(self: &Arc<Self>, id: &str, silent: bool) -> bool {
        {
            let mut st = self.lock();
            let Some(d) = st.dev_mut(id) else { return false };
            if d.connecting {
                return false;
            }
            if !silent {
                d.manual = false;
            }
            d.connecting = true;
        }
        let result = self.ble.connect(id).await;
        let ok = {
            let mut st = self.lock();
            let Some(d) = st.dev_mut(id) else { return false };
            match result {
                Ok(()) => {
                    d.connected = true;
                    d.waiting = false;
                    d.watching = false;
                    true
                }
                Err(e) => {
                    d.connected = false;
                    d.connecting = false;
                    let name = d.name.clone();
                    self.ble.drop_link(id);
                    if !silent {
                        st.toast(format!("Connection to {name} failed: {e}"));
                    }
                    false
                }
            }
        };
        if ok {
            self.write_to(id, time_packet(chrono::Local::now()), false).await;
            if let Some(d) = self.lock().dev_mut(id) {
                d.connecting = false;
            }
        }
        ok
    }

    pub fn disconnect(self: &Arc<Self>, id: &str) {
        {
            let mut st = self.lock();
            if let Some(d) = st.dev_mut(id) {
                d.manual = true;
                d.waiting = false;
                d.watching = false;
            }
            st.abort_retry(id);
        }
        let ble = self.ble.clone();
        let target = id.to_string();
        tokio::spawn(async move { ble.disconnect(&target).await });
        self.on_disconnected(id);
        self.update_scan();
    }

    pub async fn disconnect_and_wait(self: &Arc<Self>, id: &str) {
        {
            let mut st = self.lock();
            if let Some(d) = st.dev_mut(id) {
                d.manual = true;
                d.waiting = false;
                d.watching = false;
            }
            st.abort_retry(id);
        }
        self.ble.disconnect(id).await;
        self.on_disconnected(id);
    }

    pub fn forget(self: &Arc<Self>, id: &str) {
        self.disconnect(id);
        let mut st = self.lock();
        st.devices.retain(|d| d.id != id);
        st.store.devices.retain(|d| d.id != id);
        st.persist();
        if !st.devices.is_empty() && st.selected_devices().is_empty() {
            st.devices[0].selected = true;
        }
        st.sync_group();
    }

    fn on_disconnected(self: &Arc<Self>, id: &str) {
        self.ble.drop_link(id);
        let mut st = self.lock();
        let Some(d) = st.dev_mut(id) else { return };
        let was_connected = d.connected;
        d.connected = false;
        if was_connected && !d.manual {
            st.abort_retry(id);
            let me = self.clone();
            let retry_id = id.to_string();
            let handle = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                me.auto_connect(retry_id, 0);
            });
            st.retries.insert(id.to_string(), handle.abort_handle());
        }
    }

    // The [group, type] manufacturer data decides the features; it is read for a while after connecting.
    async fn read_advertisement(self: &Arc<Self>, id: &str) {
        if let Some(d) = self.lock().dev_mut(id) {
            d.advertisement_until = Some(Instant::now() + ADVERTISEMENT_WINDOW);
        }
        if let Some(adv) = self.ble.advertisement(id).await {
            let mut st = self.lock();
            if let Some(d) = st.dev_mut(id) {
                d.rssi = adv.rssi.or(d.rssi);
            }
            if let Some(data) = adv.manufacturer {
                st.apply_manufacturer(id, &data);
            }
        }
    }

    // ---------- transport ----------
    // Enqueues right away, so packets keep the order they were sent in; the future resolves after the write.
    pub fn write_to(self: &Arc<Self>, id: &str, bytes: Vec<u8>, confirmed: bool) -> impl Future<Output = ()> + Send + 'static + use<> {
        let rx = self.ble.enqueue(id, bytes, confirmed);
        let me = self.clone();
        let id = id.to_string();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            if let Ok(Err(e)) = rx.await {
                let mut st = me.lock();
                let name = st.dev(&id).map(|d| d.name.clone()).unwrap_or(id);
                let text = format!("write error on {name}: {e}");
                if st.echo_toast && !st.echo_log {
                    eprintln!("{text}");
                }
                st.push_log(text, true);
            }
            let _ = done_tx.send(());
        });
        async move {
            let _ = done_rx.await;
        }
    }

    // from_segments: packets of the static segments feature (sent with BLE confirmation when possible).
    pub fn send(self: &Arc<Self>, bytes: Vec<u8>, from_segments: bool) -> impl Future<Output = ()> + Send + 'static + use<> {
        let targets = self.lock().target_devices();
        self.send_to(targets, bytes, from_segments)
    }

    // Like send, to the given connected lamps instead of the selected ones.
    fn send_to(self: &Arc<Self>, targets: Vec<String>, bytes: Vec<u8>, from_segments: bool) -> impl Future<Output = ()> + Send + 'static + use<> {
        let mut st = self.lock();
        let confirmed = from_segments || st.confirm_writes;
        // Segments shrink the IC length; any other lighting command (power, colour, mode, rhythm,
        // motor, laser) must drive the whole strip again, so restore the full length first
        // on every target lamp that was shortened. Only there: the lamp flashes on every 0F.
        let shortened: Vec<String> = targets.iter().filter(|id| st.dev(id).map(|d| d.shortened).unwrap_or(false)).cloned().collect();
        let mut writes = Vec::new();
        if !from_segments && !shortened.is_empty() && LIGHTING_OPCODES.contains(&bytes[0]) {
            let full = st.store.settings.ic_length;
            if (MIN_LEDS..=MAX_LEDS).contains(&full) {
                st.set_shortened(&shortened, false);
                let len_packet = ic_length_packet(full as u16);
                st.push_log(to_hex(&len_packet), false);
                for id in &shortened {
                    writes.push(self.write_to(id, len_packet.clone(), st.confirm_writes));
                }
            }
        }
        st.push_log(to_hex(&bytes), targets.is_empty());
        if targets.is_empty() && st.toast_text().is_none() {
            let text = if st.devices.is_empty() { "Connect a lamp first." } else { "Select a connected lamp." };
            st.toast(text);
        }
        for id in &targets {
            writes.push(self.write_to(id, bytes.clone(), confirmed));
        }
        drop(st);
        async move {
            join_all(writes).await;
        }
    }

    pub fn fire(self: &Arc<Self>, bytes: Vec<u8>) {
        let f = self.send(bytes, false);
        tokio::spawn(f);
    }

    // Coalesces fast slider/picker updates: only the newest packet per key is sent.
    pub fn send_latest(self: &Arc<Self>, key: &'static str, bytes: Vec<u8>) {
        {
            let mut st = self.lock();
            st.pending.insert(key, bytes);
            if st.flushing.contains(key) {
                return;
            }
            st.flushing.insert(key);
        }
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                let next = {
                    let mut st = me.lock();
                    let next = st.pending.remove(key);
                    if next.is_none() {
                        st.flushing.remove(key);
                    }
                    next
                };
                let Some(bytes) = next else { break };
                me.send(bytes, false).await;
                tokio::time::sleep(Duration::from_millis(60)).await;
            }
        });
    }

    // Resolves when every coalesced sender has drained.
    pub async fn idle(self: &Arc<Self>) {
        loop {
            {
                let st = self.lock();
                if st.flushing.is_empty() && st.pending.is_empty() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    // ---------- power & colour ----------
    pub fn toggle_power(self: &Arc<Self>) {
        let on = {
            let mut st = self.lock();
            st.power = !st.power;
            st.power
        };
        self.set_power(on);
    }

    pub fn set_power(self: &Arc<Self>, on: bool) {
        self.lock().power = on;
        self.stop_phone_mic();
        self.fire(power_packet(on));
    }

    pub fn set_brightness(self: &Arc<Self>, b: u8) {
        let b = b.min(100);
        {
            let mut st = self.lock();
            st.power = true;
            st.store.settings.brightness = b;
            st.persist();
        }
        self.send_latest("bright", brightness_packet(b));
    }

    pub fn send_rgb(self: &Arc<Self>) {
        let rgb = {
            let mut st = self.lock();
            st.power = true;
            st.rgb
        };
        self.send_latest("color", rgb_packet(rgb));
    }

    pub fn set_rgb(self: &Arc<Self>, rgb: [u8; 3]) {
        {
            let mut st = self.lock();
            st.rgb = rgb;
            let (h, s) = rgb_to_hs(rgb);
            st.hue = h;
            st.sat = s;
            st.preset_index = CLASSIC_RGB.iter().position(|c| *c == rgb);
            st.color_tab = ColorTab::Color;
        }
        self.send_rgb();
    }

    pub fn set_cct(self: &Arc<Self>, v: u8) {
        {
            let mut st = self.lock();
            st.power = true;
            st.cct = v;
        }
        self.send_latest("color", cct_packet(v));
    }

    pub fn set_wb(self: &Arc<Self>, v: u8) {
        {
            let mut st = self.lock();
            st.power = true;
            st.wb = v;
        }
        self.send_latest("color", white_packet(v));
    }

    pub fn pick_preset(self: &Arc<Self>, i: usize) {
        {
            let mut st = self.lock();
            st.preset_index = Some(i);
            st.rgb = CLASSIC_RGB[i];
            let (h, s) = rgb_to_hs(st.rgb);
            st.hue = h;
            st.sat = s;
        }
        self.send_rgb();
    }

    // Same as picking a point on the colour wheel: hue and saturation at full value.
    pub fn pick_hue_sat(self: &Arc<Self>, hue: f64, sat: f64) {
        {
            let mut st = self.lock();
            st.hue = hue.rem_euclid(360.0);
            st.sat = sat.clamp(0.0, 1.0);
            st.rgb = hsv_to_rgb(st.hue, st.sat, 1.0);
            st.preset_index = None;
        }
        self.send_rgb();
    }

    pub fn set_motor(self: &Arc<Self>) {
        let p = {
            let mut st = self.lock();
            st.power = true;
            motor_packet(st.motor_on, st.motor_speed)
        };
        self.send_latest("motor", p);
    }

    pub fn set_laser(self: &Arc<Self>) {
        let p = {
            let mut st = self.lock();
            st.power = true;
            laser_packet(st.laser_on, st.laser_speed)
        };
        self.send_latest("laser", p);
    }

    // ---------- modes ----------
    pub fn pick_scene(&self, i: usize) {
        let mut st = self.lock();
        st.scene_tab = i;
        st.mode_index = None;
    }

    pub fn pick_mode(self: &Arc<Self>, i: usize) {
        self.lock().mode_index = Some(i);
        self.send_mode();
    }

    pub fn send_mode(self: &Arc<Self>) {
        let p = {
            let mut st = self.lock();
            let items = st.mode_items();
            let Some(m) = st.mode_index.and_then(|i| items.get(i)) else { return };
            let p = mode_packet(m, st.speed, st.mode_brightness);
            st.power = true;
            p
        };
        self.send_latest("mode", p);
    }

    // ---------- static segments ----------
    pub async fn apply_segments(self: &Arc<Self>) {
        let steps = {
            let mut st = self.lock();
            if let Some(err) = st.segments_error() {
                st.toast(err);
                return;
            }
            if st.segments_busy {
                return;
            }
            st.segments_busy = true;
            st.persist();
            st.power = true;
            st.segment_steps()
        };
        // Behave like clicking each row by hand: one segment at a time with a pause in between,
        // otherwise the lamp misses some of the segments.
        for (i, step) in steps.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(Duration::from_millis(SEGMENT_PAUSE_MS)).await;
            }
            for p in &step.packets {
                self.send(p.clone(), true).await;
            }
        }
        let mut st = self.lock();
        let active = st.store.segments.len() > 1;
        let targets = st.target_devices();
        st.set_shortened(&targets, active);
        st.segments_busy = false;
    }

    // Sends one segment on its own: its length (full length for the top one) and its colour.
    pub async fn apply_segment(self: &Arc<Self>, i: usize) {
        let step = {
            let mut st = self.lock();
            if let Some(err) = st.segments_error() {
                st.toast(err);
                return;
            }
            st.persist();
            let Some(step) = st.segment_steps().into_iter().find(|s| s.index == i) else { return };
            st.power = true;
            let active = i < st.store.segments.len() - 1;
            let targets = st.target_devices();
            st.set_shortened(&targets, active);
            step
        };
        for p in step.packets {
            self.send(p, true).await;
        }
    }

    pub fn restore_length(self: &Arc<Self>) {
        let full = {
            let mut st = self.lock();
            let full = st.store.settings.ic_length;
            if !(MIN_LEDS..=MAX_LEDS).contains(&full) {
                st.toast("Enter the full LED count of the strip (16–2048).");
                return;
            }
            let targets = st.target_devices();
            st.set_shortened(&targets, false);
            full
        };
        self.fire(ic_length_packet(full as u16));
    }

    // ---------- favourites ----------
    pub async fn apply_favorite(self: &Arc<Self>, f: &Favorite) {
        self.lock().restore_favorite_ui(f);
        if matches!(f.kind, FavoriteKind::Segments { .. }) {
            self.apply_segments().await;
            return;
        }
        let targets = self.lock().target_devices();
        self.play_favorite(targets, f).await;
    }

    // Sends a favourite to the given lamps. Segments go one at a time with the pause in between.
    async fn play_favorite(self: &Arc<Self>, targets: Vec<String>, f: &Favorite) {
        match &f.kind {
            FavoriteKind::Color { packets, .. } | FavoriteKind::Mode { packets, .. } => {
                for p in packets {
                    self.send_to(targets.clone(), p.clone(), false).await;
                }
            }
            FavoriteKind::Segments { ic_length, segments, .. } => {
                let segments: Vec<Segment> = segments.iter().map(|s| Segment { color: s.color, end: s.end }).collect();
                if let Some(err) = segments::segments_error(&segments, *ic_length) {
                    self.lock().toast(err);
                    return;
                }
                for (i, step) in segments::segment_steps(&segments, *ic_length).iter().enumerate() {
                    if i > 0 {
                        tokio::time::sleep(Duration::from_millis(SEGMENT_PAUSE_MS)).await;
                    }
                    for p in &step.packets {
                        self.send_to(targets.clone(), p.clone(), true).await;
                    }
                }
                self.lock().set_shortened(&targets, segments.len() > 1);
            }
        }
        let mut st = self.lock();
        for id in &targets {
            if let Some(d) = st.dev_mut(id) {
                d.last_favorite = f.id.clone();
            }
        }
    }

    // All lamps of the setup at the same time; each lamp gets its own favourite (or off).
    pub async fn apply_setup(self: &Arc<Self>, s: &Setup) {
        let mut skipped = Vec::new();
        let mut tasks = Vec::new();
        for lamp in &s.lamps {
            let (connected, name, fav) = {
                let st = self.lock();
                let d = st.dev(&lamp.device);
                (
                    d.map(|d| d.connected).unwrap_or(false),
                    d.map(|d| d.name.clone()).unwrap_or_else(|| "unknown lamp".to_string()),
                    st.store.favorites.iter().find(|f| f.id == lamp.action).cloned()
                )
            };
            if !connected {
                skipped.push(name);
                continue;
            }
            let targets = vec![lamp.device.clone()];
            if lamp.action == "off" {
                tasks.push(self.send_to(targets, power_packet(false), false).boxed());
                continue;
            }
            if let Some(f) = fav {
                // Same order as one by one: the last lamp's favourite stays on screen.
                self.lock().restore_favorite_ui(&f);
                let me = self.clone();
                tasks.push(async move { me.play_favorite(targets, &f).await }.boxed());
            }
        }
        join_all(tasks).await;
        if !skipped.is_empty() {
            self.lock().toast(format!("Not connected, skipped: {}", skipped.join(", ")));
        }
    }

    // ---------- rhythm ----------
    pub fn set_mic_source(self: &Arc<Self>, src: MicSource) {
        {
            let mut st = self.lock();
            if st.mic_source == src {
                return;
            }
            st.mic_source = src;
        }
        if src == MicSource::Device {
            self.stop_phone_mic();
            self.send_device_mic();
        }
    }

    pub fn pick_effect(self: &Arc<Self>, i: u8) {
        let device = {
            let mut st = self.lock();
            st.mic_effect = i;
            st.mic_source == MicSource::Device
        };
        if device {
            self.send_device_mic();
        }
    }

    pub fn send_device_mic(self: &Arc<Self>) {
        let p = {
            let mut st = self.lock();
            st.power = true;
            device_mic_packet(st.mic_effect, st.sensitivity)
        };
        self.send_latest("mic", p);
    }

    pub fn set_mic_color(&self, c: i32) {
        let mut st = self.lock();
        st.store.settings.mic_color = c;
        st.persist();
    }

    pub fn start_phone_mic(self: &Arc<Self>) {
        if self.lock().recording {
            return;
        }
        let capture = match MicCapture::start() {
            Ok(c) => c,
            Err(_) => {
                self.lock().toast("No access to the microphone.");
                return;
            }
        };
        *self.mic.lock().unwrap() = Some(capture);
        let generation = {
            let mut st = self.lock();
            st.recording = true;
            st.power = true;
            st.mic_generation += 1;
            st.mic_generation
        };
        self.fire(phone_start_packet());
        let me = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(150));
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if !me.mic_tick(generation) {
                    break;
                }
            }
        });
    }

    fn mic_tick(self: &Arc<Self>, generation: u64) -> bool {
        let samples = match self.mic.lock().unwrap().as_ref() {
            Some(m) => m.samples(),
            None => return false
        };
        let frame = {
            let mut st = self.lock();
            if !st.recording || st.mic_generation != generation {
                return false;
            }
            let vol = mic_volume(&samples);
            st.level = js_round(vol as f64 / 255.0 * 100.0) as u8;
            let c = st.current_mic_color();
            st.mic_color_index = (st.mic_color_index + 1) % MIC_COLORS.len();
            phone_frame_packet(c, vol, st.mic_effect)
        };
        self.send_latest("phone", frame);
        true
    }

    pub fn stop_phone_mic(self: &Arc<Self>) {
        let frame = {
            let mut st = self.lock();
            if !st.recording {
                return;
            }
            st.recording = false;
            st.level = 0;
            phone_frame_packet(st.current_mic_color(), 0, st.mic_effect)
        };
        self.mic.lock().unwrap().take();
        self.send_latest("phone", frame);
    }

    // ---------- timers ----------
    pub fn save_alarm(self: &Arc<Self>) {
        {
            let mut st = self.lock();
            let Some(f) = st.alarm_form.clone() else { return };
            let Some((hour, minute)) = parse_time(&f.time) else {
                st.toast("Enter the time.");
                return;
            };
            let a = Alarm { hour, minute, weeks: f.weeks, on: f.on, enable: f.enable };
            match f.index {
                Some(i) if i < st.store.alarms.len() => st.store.alarms[i] = a,
                _ => st.store.alarms.push(a)
            }
            st.alarm_form = None;
        }
        self.sync_alarms();
    }

    pub fn remove_alarm(self: &Arc<Self>, i: usize) {
        {
            let mut st = self.lock();
            if i < st.store.alarms.len() {
                st.store.alarms.remove(i);
            }
            st.alarm_form = None;
        }
        self.sync_alarms();
    }

    pub fn toggle_alarm(self: &Arc<Self>, i: usize) {
        {
            let mut st = self.lock();
            let Some(a) = st.store.alarms.get_mut(i) else { return };
            a.enable = !a.enable;
        }
        self.sync_alarms();
    }

    pub fn sync_alarms(self: &Arc<Self>) {
        let p = {
            let st = self.lock();
            st.persist();
            alarms_packet(&st.store.alarms)
        };
        self.fire(p);
    }

    pub fn save_delay(self: &Arc<Self>) {
        {
            let mut st = self.lock();
            let Some(f) = st.delay_form.clone() else { return };
            let hour = f.hour.clamp(0, 23) as u8;
            let minute = f.minute.clamp(0, 59) as u8;
            if hour == 0 && minute == 0 {
                st.toast("Set at least 1 minute.");
                return;
            }
            st.store.delay = Some(Delay { hour, minute, enable: f.enable });
            st.delay_form = None;
        }
        self.sync_delay();
    }

    pub fn toggle_delay(self: &Arc<Self>) {
        {
            let mut st = self.lock();
            let Some(d) = st.store.delay.as_mut() else { return };
            d.enable = !d.enable;
        }
        self.sync_delay();
    }

    pub fn remove_delay(self: &Arc<Self>) {
        let enabled = self.lock().store.delay.as_ref().map(|d| d.enable).unwrap_or(false);
        if enabled {
            if let Some(d) = self.lock().store.delay.as_mut() {
                d.enable = false;
            }
            self.sync_delay();
        }
        let mut st = self.lock();
        st.store.delay = None;
        st.delay_form = None;
        st.persist();
    }

    pub fn sync_delay(self: &Arc<Self>) {
        let p = {
            let st = self.lock();
            st.persist();
            let Some(d) = &st.store.delay else { return };
            delay_packet(d.enable, d.hour as u16 * 60 + d.minute as u16)
        };
        self.fire(p);
    }

    // ---------- settings ----------
    pub fn send_ic_order(self: &Arc<Self>) {
        let order = {
            let st = self.lock();
            st.persist();
            st.store.settings.ic_order
        };
        self.fire(ic_order_packet(order));
    }

    pub fn send_ic_length(self: &Arc<Self>) {
        let n = {
            let mut st = self.lock();
            let n = st.store.settings.ic_length;
            if !(MIN_LEDS..=MAX_LEDS).contains(&n) {
                st.toast("The LED count must be between 16 and 2048.");
                return;
            }
            st.persist();
            n
        };
        self.fire(ic_length_packet(n as u16));
    }

    pub fn sync_time(self: &Arc<Self>, id: &str) -> impl Future<Output = ()> + Send + 'static + use<> {
        let p = time_packet(chrono::Local::now());
        let f = self.write_to(id, p.clone(), false);
        self.lock().push_log(to_hex(&p), false);
        f
    }

    pub fn shutdown(self: &Arc<Self>) {
        self.stop_phone_mic();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_parsing() {
        assert_eq!(parse_time("7:05"), Some((7, 5)));
        assert_eq!(parse_time("25:99"), Some((23, 59)));
        assert_eq!(parse_time("12:3"), None);
        assert_eq!(parse_time(""), None);
    }
}
