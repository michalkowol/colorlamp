use crate::core::{AlarmForm, MainTab, MicSource, Shared, State};
use crate::protocol::*;
use crate::store::FavoriteKind;
use anyhow::Result;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEventKind
};
use futures::StreamExt;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Widget, Wrap};
use std::time::Duration;

const CLAY: Color = Color::Rgb(0xD9, 0x77, 0x57);
const OLIVE: Color = Color::Rgb(0x78, 0x8C, 0x5D);
const GRAY: Color = Color::Rgb(0x87, 0x86, 0x7F);
const IVORY: Color = Color::Rgb(0xFA, 0xF9, 0xF5);
const BAR: usize = 32;
const WHEEL_W: u16 = 30;
const WHEEL_H: u16 = 15;

const TABS: [(MainTab, &str); 8] = [
    (MainTab::Devices, "Devices"),
    (MainTab::Color, "Colour"),
    (MainTab::Mode, "Modes"),
    (MainTab::Rhythm, "Rhythm"),
    (MainTab::Timer, "Schedule"),
    (MainTab::Settings, "Settings"),
    (MainTab::Favorites, "Favourites"),
    (MainTab::Setups, "Setups")
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctl {
    AddLamp,
    Device(usize),

    ColorTab,
    Hue,
    Sat,
    Presets,
    Hex,
    Cct,
    CctPresets,
    Wb,
    WbPresets,
    MotorOn,
    MotorSpeed,
    LaserOn,
    LaserSpeed,
    SegLength,
    SegColor(usize),
    SegEnd(usize),
    SegApply(usize),
    SegRemove(usize),
    SegApplyAll,
    SegAdd,
    SegSplit,
    SegFav,
    SegRestore,
    Brightness,
    ColorFav,

    Scene,
    ModeGrid,
    Speed,
    ModeBrightness,
    ModeFav,

    MicSource,
    Effect,
    Sensitivity,
    Listen,
    MicColor,

    AlarmAdd,
    Alarm(usize),
    AlarmTime,
    AlarmAction,
    AlarmDays,
    AlarmActive,
    AlarmSave,
    AlarmCancel,
    AlarmDelete,
    DelaySet,
    DelayActive,
    DelayEdit,
    DelayDelete,
    DelayHour,
    DelayMinute,
    DelayFormActive,
    DelaySave,
    DelayCancel,

    Order,
    OrderSend,
    IcLength,
    IcLengthSend,
    SyncTime(usize),

    Favorite(usize),

    NewSetup,
    Setup(usize),
    SetupName,
    SetupLamp(usize),
    SetupSave,
    SetupCancel
}

struct Row {
    ctl: Option<Ctl>,
    lines: Vec<Line<'static>>
}

#[derive(Clone, Debug)]
enum PromptTarget {
    RenameDevice(String),
    RenameFavorite(String),
    RenameSetup(String),
    Hex,
    Slider(Ctl),
    SegLength,
    SegEnd(usize),
    IcLength,
    AlarmTime,
    DelayHour,
    DelayMinute,
    SetupName
}

struct Prompt {
    title: String,
    value: String,
    target: PromptTarget
}

struct Ui {
    focus: Option<Ctl>,
    focus_idx: usize,
    scroll: usize,
    cursor: usize,
    cursor_for: Option<Ctl>,
    grid_cols: usize,
    prompt: Option<Prompt>,
    chooser: Option<usize>,
    wheel: Option<Rect>,
    help: bool,
    last_tab: MainTab
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Quit
}

pub async fn run(core: Shared) -> Result<()> {
    core.start().await;
    core.restore_devices();
    let mut terminal = ratatui::init();
    crossterm::execute!(std::io::stdout(), EnableMouseCapture)?;
    let mut ui = Ui {
        focus: None,
        focus_idx: 0,
        scroll: 0,
        cursor: 0,
        cursor_for: None,
        grid_cols: 1,
        prompt: None,
        chooser: None,
        wheel: None,
        help: false,
        last_tab: core.lock().tab
    };
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let result: Result<()> = loop {
        ui.follow_tab(&core);
        if let Err(e) = terminal.draw(|f| ui.draw(f, &core)) {
            break Err(e.into());
        }
        tokio::select! {
            _ = tick.tick() => {}
            ev = events.next() => match ev {
                Some(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => {
                    if ui.on_key(&core, key) == Flow::Quit {
                        break Ok(());
                    }
                }
                Some(Ok(Event::Mouse(m))) => {
                    if matches!(m.kind, MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)) {
                        ui.on_click(&core, m.column, m.row);
                    }
                    if matches!(m.kind, MouseEventKind::ScrollDown) {
                        ui.move_focus(&core, 1);
                    }
                    if matches!(m.kind, MouseEventKind::ScrollUp) {
                        ui.move_focus(&core, -1);
                    }
                }
                Some(Err(e)) => break Err(e.into()),
                None => break Ok(()),
                _ => {}
            }
        }
    };
    core.shutdown();
    let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

fn rgb(c: [u8; 3]) -> Color {
    Color::Rgb(c[0], c[1], c[2])
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(GRAY))
}

fn label(text: &str) -> Span<'static> {
    Span::raw(format!("{text:<28}"))
}

fn heading(text: &str) -> Row {
    Row { ctl: None, lines: vec![Line::from(Span::styled(text.to_string(), Style::default().add_modifier(Modifier::BOLD)))] }
}

fn note(text: impl Into<String>) -> Row {
    Row { ctl: None, lines: vec![Line::from(dim(text))] }
}

fn warn(text: impl Into<String>) -> Row {
    Row { ctl: None, lines: vec![Line::from(Span::styled(text.into(), Style::default().fg(CLAY)))] }
}

fn blank() -> Row {
    Row { ctl: None, lines: vec![Line::raw("")] }
}

fn button(ctl: Ctl, text: &str, enabled: bool) -> Row {
    let style = if enabled { Style::default() } else { Style::default().fg(GRAY).add_modifier(Modifier::CROSSED_OUT) };
    Row { ctl: Some(ctl), lines: vec![Line::from(Span::styled(format!("[ {text} ]"), style))] }
}

fn toggle(ctl: Ctl, text: &str, on: bool) -> Row {
    let mark = if on { Span::styled("[x] ", Style::default().fg(CLAY)) } else { Span::raw("[ ] ") };
    Row { ctl: Some(ctl), lines: vec![Line::from(vec![mark, Span::raw(text.to_string())])] }
}

fn field(ctl: Ctl, name: &str, value: String) -> Row {
    Row { ctl: Some(ctl), lines: vec![Line::from(vec![label(name), Span::styled(format!("‹{value}›"), Style::default().fg(IVORY))])] }
}

fn choice(ctl: Ctl, name: &str, options: &[String], index: Option<usize>) -> Row {
    let mut spans = vec![label(name)];
    for (i, o) in options.iter().enumerate() {
        if Some(i) == index {
            spans.push(Span::styled(format!(" {o} "), Style::default().fg(Color::Black).bg(IVORY)));
        } else {
            spans.push(dim(format!(" {o} ")));
        }
    }
    Row { ctl: Some(ctl), lines: vec![Line::from(spans)] }
}

// One-line choice that shows only the current option, for long lists.
fn spinner(ctl: Ctl, name: &str, value: String) -> Row {
    Row { ctl: Some(ctl), lines: vec![Line::from(vec![label(name), Span::styled(format!("◀ {value} ▶"), Style::default().fg(IVORY))])] }
}

fn slider(ctl: Ctl, name: &str, value: i64, min: i64, max: i64, suffix: &str, gradient: Option<&dyn Fn(f64) -> Color>) -> Row {
    let t = if max > min { (value - min) as f64 / (max - min) as f64 } else { 0.0 };
    let knob = js_round(t * (BAR - 1) as f64) as usize;
    let mut spans = vec![label(name)];
    for x in 0..BAR {
        let pos = x as f64 / (BAR - 1) as f64;
        match gradient {
            Some(g) => {
                let c = g(pos);
                if x == knob {
                    spans.push(Span::styled("█", Style::default().fg(Color::Black).bg(c)));
                    spans.last_mut().unwrap().content = "◆".into();
                } else {
                    spans.push(Span::styled("▆", Style::default().fg(c)));
                }
            }
            None => {
                if x == knob {
                    spans.push(Span::styled("●", Style::default().fg(IVORY)));
                } else if x < knob {
                    spans.push(Span::styled("━", Style::default().fg(CLAY)));
                } else {
                    spans.push(Span::styled("─", Style::default().fg(GRAY)));
                }
            }
        }
    }
    spans.push(Span::raw(format!("  {value}{suffix}")));
    Row { ctl: Some(ctl), lines: vec![Line::from(spans)] }
}

struct Chip {
    color: Option<[u8; 3]>,
    text: String,
    title: String
}

fn chips(ctl: Ctl, name: &str, list: &[Chip], selected: Option<usize>, cursor: Option<usize>) -> Row {
    let mut spans = vec![label(name)];
    for (i, chip) in list.iter().enumerate() {
        let marker = match (Some(i) == selected, Some(i) == cursor) {
            (true, true) => Span::styled("◆", Style::default().fg(CLAY)),
            (true, false) => Span::styled("•", Style::default().fg(IVORY)),
            (false, true) => Span::styled("›", Style::default().fg(CLAY).add_modifier(Modifier::BOLD)),
            (false, false) => Span::raw(" ")
        };
        spans.push(marker);
        match chip.color {
            Some(c) => spans.push(Span::styled(chip.text.clone(), Style::default().fg(rgb(c)))),
            None => spans.push(Span::raw(chip.text.clone()))
        }
    }
    if let Some(c) = cursor.and_then(|c| list.get(c)) {
        spans.push(dim(format!("  {}", c.title)));
    } else if let Some(c) = selected.and_then(|c| list.get(c)) {
        spans.push(dim(format!("  {}", c.title)));
    }
    Row { ctl: Some(ctl), lines: vec![Line::from(spans)] }
}

fn palette_chips() -> Vec<Chip> {
    PALETTE_ORDER
        .iter()
        .map(|i| Chip { color: Some(CLASSIC_RGB[*i]), text: "██".into(), title: CLASSIC_NAMES[*i].into() })
        .collect()
}

fn dots(list: &[[u8; 3]]) -> Vec<Span<'static>> {
    list.iter().map(|c| Span::styled("● ", Style::default().fg(rgb(*c)))).collect()
}

fn hue_color(t: f64) -> Color {
    rgb(hsv_to_rgb(t * 359.0, 1.0, 1.0))
}

fn segments_label(i: usize, n: usize) -> String {
    let suffix = if i == n - 1 {
        " (top)"
    } else if i == 0 {
        " (bottom)"
    } else {
        ""
    };
    format!("Segment {}{suffix}", n - i)
}

fn mode_cell(item: &ModeItem, width: usize) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = item.dots.iter().map(|d| Span::styled("●", Style::default().fg(rgb(d.rgb)))).collect();
    let used = item.dots.len();
    let text = if item.label.is_empty() { String::new() } else { format!(" {}", item.label) };
    let room = width.saturating_sub(used + 3);
    let text: String = text.chars().take(room).collect();
    let pad = width.saturating_sub(used + text.chars().count() + 2);
    spans.push(dim(text));
    spans.push(Span::raw(" ".repeat(pad)));
    spans
}

impl Ui {
    fn follow_tab(&mut self, core: &Shared) {
        let tab = {
            let mut st = core.lock();
            st.sync_group();
            st.tab
        };
        if tab != self.last_tab {
            if self.last_tab == MainTab::Rhythm {
                core.stop_phone_mic();
            }
            self.last_tab = tab;
            self.focus = None;
            self.focus_idx = 0;
            self.scroll = 0;
        }
    }

    fn set_tab(&mut self, core: &Shared, tab: MainTab) {
        core.lock().tab = tab;
        self.follow_tab(core);
    }

    fn rows(&mut self, st: &State, width: usize) -> Vec<Row> {
        match st.tab {
            MainTab::Devices => self.devices_rows(st),
            MainTab::Color => self.color_rows(st),
            MainTab::Mode => self.mode_rows(st, width),
            MainTab::Rhythm => self.rhythm_rows(st),
            MainTab::Timer => self.timer_rows(st),
            MainTab::Settings => self.settings_rows(st),
            MainTab::Favorites => self.favorite_rows(st),
            MainTab::Setups => self.setup_rows(st)
        }
    }

    fn cursor_of(&self, ctl: Ctl) -> Option<usize> {
        if self.focus == Some(ctl) && self.cursor_for == Some(ctl) { Some(self.cursor) } else { None }
    }

    // ---------- tabs ----------
    fn devices_rows(&self, st: &State) -> Vec<Row> {
        let mut rows = vec![
            Row {
                ctl: None,
                lines: vec![Line::from(vec![
                    Span::styled("Devices  ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{} connected", st.connected_count()), Style::default().fg(if st.connected_count() > 0 { OLIVE } else { GRAY })),
                ])]
            },
            button(Ctl::AddLamp, "Add lamp", true),
        ];
        if let Some(e) = &st.ble_error {
            rows.push(warn(e.clone()));
        }
        if st.devices.is_empty() {
            rows.push(note("No devices. Turn the lamp on, choose Add lamp and pick it from the list."));
        }
        for (i, d) in st.devices.iter().enumerate() {
            let state = if d.connected {
                Span::styled("● ", Style::default().fg(OLIVE))
            } else if d.connecting || d.waiting {
                Span::styled("● ", Style::default().fg(CLAY))
            } else {
                dim("● ")
            };
            let check = if d.selected { Span::styled("[x] ", Style::default().fg(CLAY)) } else { Span::raw("[ ] ") };
            let group = match d.group {
                Some(g) => group_name(g),
                None => "type: unknown".to_string()
            };
            let mut sub = String::new();
            if d.name != d.ble_name {
                sub.push_str(&format!("{} · ", d.ble_name));
            }
            sub.push_str(d.status());
            if let Some(r) = d.rssi {
                sub.push_str(&format!(" · {r} dBm"));
            }
            if d.shortened {
                sub.push_str(" · LED count shortened");
            }
            rows.push(Row {
                ctl: Some(Ctl::Device(i)),
                lines: vec![
                    Line::from(vec![
                        check,
                        state,
                        Span::styled(format!("{:<28}", d.name), Style::default().add_modifier(Modifier::BOLD)),
                        Span::raw(format!("◀ {group} ▶")),
                    ]),
                    Line::from(vec![Span::raw("        "), dim(sub)]),
                ]
            });
        }
        if !st.devices.is_empty() && !st.group_known() {
            rows.push(blank());
            rows.push(note(
                "The lamp type is not always in the BLE advertisement. If it is missing, pick the type by hand to match the available features (default: Magic, addressable)."
            ));
        }
        rows.push(blank());
        rows.push(note("The lamp never reports its state; the app only sends commands. Lamps added before connect by themselves and reconnect when they come back in range."));
        rows
    }

    fn color_rows(&self, st: &State) -> Vec<Row> {
        let mut rows = Vec::new();
        let tabs = st.color_tabs();
        if tabs.len() > 1 {
            let names: Vec<String> = tabs.iter().map(|t| t.name().to_string()).collect();
            rows.push(choice(Ctl::ColorTab, "Mode", &names, tabs.iter().position(|t| *t == st.color_tab)));
            rows.push(blank());
        }
        match st.color_tab {
            ColorTab::Color => {
                rows.push(Row {
                    ctl: None,
                    lines: vec![Line::from(vec![
                        label("Selected colour"),
                        Span::styled("      ", Style::default().bg(rgb(st.rgb))),
                        Span::raw(format!("  R {} · G {} · B {}", st.rgb[0], st.rgb[1], st.rgb[2])),
                    ])]
                });
                let sat_end = rgb(hsv_to_rgb(st.hue, 1.0, 1.0));
                let sat_grad = move |t: f64| -> Color {
                    let Color::Rgb(r, g, b) = sat_end else { return Color::White };
                    let mix = |c: u8| (255.0 + (c as f64 - 255.0) * t) as u8;
                    Color::Rgb(mix(r), mix(g), mix(b))
                };
                rows.push(slider(Ctl::Hue, "Hue", js_round(st.hue) as i64, 0, 359, "°", Some(&hue_color)));
                rows.push(slider(Ctl::Sat, "Saturation", js_round(st.sat * 100.0) as i64, 0, 100, "%", Some(&sat_grad)));
                let sel = st.preset_index.and_then(|p| PALETTE_ORDER.iter().position(|x| *x == p));
                rows.push(chips(Ctl::Presets, "Presets", &palette_chips(), sel, self.cursor_of(Ctl::Presets)));
                rows.push(field(Ctl::Hex, "Hex", format!("#{:02x}{:02x}{:02x}", st.rgb[0], st.rgb[1], st.rgb[2])));
                rows.push(note("On Magic strips only fully saturated colours look right. Click the wheel to pick a colour."));
            }
            ColorTab::Cct => {
                let grad = |t: f64| Color::Rgb(255, 255, (t * 255.0) as u8);
                rows.push(slider(Ctl::Cct, "Temperature: warm → cold", st.cct as i64, 0, 255, "", Some(&grad)));
                let presets = cct_presets();
                let list: Vec<Chip> =
                    presets.iter().map(|v| Chip { color: Some([255, 255, *v]), text: "██".into(), title: v.to_string() }).collect();
                rows.push(chips(Ctl::CctPresets, "Presets", &list, presets.iter().position(|v| *v == st.cct), self.cursor_of(Ctl::CctPresets)));
            }
            ColorTab::Wb => {
                let grad = |t: f64| {
                    let v = (t * 255.0) as u8;
                    Color::Rgb(v, v, v)
                };
                rows.push(slider(Ctl::Wb, "White channel", st.wb as i64, 0, 255, "", Some(&grad)));
                let presets = wb_presets();
                let list: Vec<Chip> =
                    presets.iter().map(|v| Chip { color: Some([*v, *v, *v]), text: "██".into(), title: v.to_string() }).collect();
                rows.push(chips(Ctl::WbPresets, "Presets", &list, presets.iter().position(|v| *v == st.wb), self.cursor_of(Ctl::WbPresets)));
            }
            ColorTab::Motor => {
                rows.push(toggle(Ctl::MotorOn, "Rotation on", st.motor_on));
                rows.push(slider(Ctl::MotorSpeed, "Speed", st.motor_speed as i64, 0, 100, "", None));
            }
            ColorTab::Laser => {
                rows.push(toggle(Ctl::LaserOn, "Stars (laser) on", st.laser_on));
                rows.push(slider(Ctl::LaserSpeed, "Speed", st.laser_speed as i64, 0, 100, "", None));
            }
            ColorTab::Segments => rows.extend(self.segment_rows(st))
        }
        rows.push(blank());
        rows.push(slider(Ctl::Brightness, "Brightness", st.store.settings.brightness as i64, 0, 100, "%", None));
        if st.can_save_color() {
            rows.push(button(Ctl::ColorFav, "☆ Add to favourites", true));
        }
        rows
    }

    fn segment_rows(&self, st: &State) -> Vec<Row> {
        let mut rows = vec![note(
            "Fixed colours on parts of the strip, without animation. The lamp has no per-LED command, so the app paints the whole strip with the top colour, then shrinks the number of controlled LEDs and paints the next segments downwards. LEDs above that number keep their colour."
        )];
        let full = st.store.settings.ic_length;
        rows.push(field(Ctl::SegLength, "Full LED count of the strip", full.to_string()));
        let error = st.segments_error();
        let busy = st.segments_busy;
        let n = st.store.segments.len();
        for i in (0..n).rev() {
            let seg = &st.store.segments[i];
            rows.push(blank());
            let sel = PALETTE_ORDER.iter().position(|c| *c == seg.color);
            rows.push(chips(Ctl::SegColor(i), &segments_label(i, n), &palette_chips(), sel, self.cursor_of(Ctl::SegColor(i))));
            let percent = st.segment_percent(i);
            if i < n - 1 {
                let mut row = field(Ctl::SegEnd(i), "  Up to LED", seg.end.map(|e| e.to_string()).unwrap_or_default());
                row.lines[0].spans.push(dim(format!("   {percent}")));
                rows.push(row);
            } else {
                rows.push(Row {
                    ctl: None,
                    lines: vec![Line::from(vec![label("  Up to LED"), Span::raw(if full > 0 { full.to_string() } else { "—".into() }), dim(format!("   {percent}"))])]
                });
            }
            rows.push(button(Ctl::SegApply(i), "Apply (only this segment)", !busy && error.is_none()));
            if n > 2 {
                rows.push(button(Ctl::SegRemove(i), "Remove segment", true));
            }
        }
        rows.push(blank());
        match &error {
            None => {
                rows.push(note("Steps to be sent"));
                for (k, step) in st.segment_steps().iter().enumerate() {
                    let hex = step.packets.iter().map(|p| to_hex(p)).collect::<Vec<_>>().join("  ·  ");
                    rows.push(Row {
                        ctl: None,
                        lines: vec![Line::from(vec![Span::raw(format!("  {}. {} ", k + 1, step.label)), Span::styled(hex, Style::default().fg(OLIVE))])]
                    });
                }
            }
            Some(e) => rows.push(warn(e.clone()))
        }
        rows.push(blank());
        rows.push(button(Ctl::SegApplyAll, if busy { "Sending…" } else { "Apply all" }, !busy && error.is_none()));
        rows.push(button(Ctl::SegAdd, "Add segment", n < 6));
        rows.push(button(Ctl::SegSplit, "Split evenly", true));
        rows.push(button(Ctl::SegFav, "☆ Add to favourites", error.is_none()));
        rows.push(button(Ctl::SegRestore, "Restore full length", true));
        rows.push(note(
            "The shortest bottom segment is 16 LEDs (controller limit). When done, choose Restore full length, otherwise modes only run on part of the strip. Any other colour, mode or power command also restores it."
        ));
        if st.segments_active() {
            rows.push(warn("The number of controlled LEDs is currently shortened."));
        }
        rows
    }

    fn mode_rows(&mut self, st: &State, width: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let scenes = st.scene_tabs();
        if !scenes.is_empty() {
            let names: Vec<String> = scenes.iter().map(|s| s.to_string()).collect();
            rows.push(spinner(Ctl::Scene, "Scene", format!("{} ({}/{})", names[st.scene_tab.min(names.len() - 1)], st.scene_tab + 1, names.len())));
            rows.push(blank());
        }
        let items = st.mode_items();
        let cell = if st.is_two_path() { 18 } else { 24 };
        let cols = (width.saturating_sub(2) / cell).max(1);
        self.grid_cols = cols;
        let cursor = self.cursor_of(Ctl::ModeGrid);
        let mut lines = Vec::new();
        for (r, chunk) in items.chunks(cols).enumerate() {
            let mut spans = Vec::new();
            for (c, item) in chunk.iter().enumerate() {
                let i = r * cols + c;
                let active = st.mode_index == Some(i);
                let marker = match (active, cursor == Some(i)) {
                    (true, true) => Span::styled("▶", Style::default().fg(CLAY).add_modifier(Modifier::REVERSED)),
                    (true, false) => Span::styled("▶", Style::default().fg(CLAY)),
                    (false, true) => Span::styled("›", Style::default().fg(IVORY).add_modifier(Modifier::REVERSED)),
                    (false, false) => Span::raw(" ")
                };
                spans.push(marker);
                spans.push(Span::raw(" "));
                if item.dots.is_empty() {
                    let text: String = format!("{:<w$}", item.label, w = cell - 2).chars().take(cell - 2).collect();
                    spans.push(Span::raw(text));
                } else {
                    spans.extend(mode_cell(item, cell));
                }
            }
            lines.push(Line::from(spans));
        }
        rows.push(Row { ctl: Some(Ctl::ModeGrid), lines });
        rows.push(blank());
        rows.push(slider(Ctl::Speed, "Speed", st.speed as i64, 0, 100, "", None));
        rows.push(slider(Ctl::ModeBrightness, "Brightness", st.mode_brightness as i64, 0, 100, "%", None));
        rows.push(button(Ctl::ModeFav, "☆ Add to favourites", st.mode_index.is_some()));
        if st.mode_index.is_none() {
            rows.push(note("Pick an effect to start it. The sliders change the active effect."));
        }
        rows.push(note("Speed and brightness are 0-100. A mode keeps running at speed 0; there is no way to freeze it."));
        rows
    }

    fn rhythm_rows(&self, st: &State) -> Vec<Row> {
        let sources = vec!["Lamp microphone".to_string(), "This device's microphone".to_string()];
        let mut rows = vec![choice(Ctl::MicSource, "Source", &sources, Some(if st.mic_source == MicSource::Device { 0 } else { 1 }))];
        let effects: Vec<String> = st.mic_effects().iter().map(|s| s.to_string()).collect();
        rows.push(choice(Ctl::Effect, "Effect", &effects, Some(st.mic_effect as usize)));
        if st.mic_source == MicSource::Device {
            rows.push(slider(Ctl::Sensitivity, "Sensitivity", st.sensitivity as i64, 0, 100, "", None));
            rows.push(note("The lamp microphone has no colour setting."));
        } else {
            let mut row = button(Ctl::Listen, if st.recording { "Stop" } else { "Start listening" }, true);
            row.lines[0].spans.push(Span::raw("  "));
            row.lines[0].spans.push(if st.recording {
                Span::styled(" listening ", Style::default().fg(CLAY).add_modifier(Modifier::REVERSED))
            } else {
                dim(" inactive ")
            });
            rows.push(row);
            let mut list = vec![Chip { color: None, text: "Rotate".into(), title: "rotating colours like the original app".into() }];
            list.extend(palette_chips());
            let selected = if st.store.settings.mic_color < 0 {
                Some(0)
            } else {
                PALETTE_ORDER.iter().position(|c| *c as i32 == st.store.settings.mic_color).map(|p| p + 1)
            };
            rows.push(chips(Ctl::MicColor, "Colour", &list, selected, self.cursor_of(Ctl::MicColor)));
            let filled = js_round(st.level as f64 / 100.0 * BAR as f64) as usize;
            rows.push(Row {
                ctl: None,
                lines: vec![Line::from(vec![
                    label("Level"),
                    Span::styled("█".repeat(filled), Style::default().fg(CLAY)),
                    dim("░".repeat(BAR - filled.min(BAR))),
                    Span::raw(format!("  {}%", st.level)),
                ])]
            });
            rows.push(note("The microphone level is sent to the lamp every 150 ms, in the chosen colour or rotating colours. The lamp microphone has no colour setting."));
        }
        rows
    }

    fn timer_rows(&self, st: &State) -> Vec<Row> {
        let mut rows = vec![heading("Schedule"), button(Ctl::AlarmAdd, "Add", st.store.alarms.len() < 6)];
        if st.store.alarms.is_empty() && st.alarm_form.is_none() {
            rows.push(note("No schedules. The lamp stores up to 6 active entries."));
        }
        for (i, a) in st.store.alarms.iter().enumerate() {
            let style = if a.enable { Style::default() } else { Style::default().fg(GRAY) };
            rows.push(Row {
                ctl: Some(Ctl::Alarm(i)),
                lines: vec![Line::from(vec![
                    Span::styled(if a.enable { "[x] " } else { "[ ] " }, Style::default().fg(CLAY)),
                    Span::styled(format!("{:02}:{:02}  ", a.hour, a.minute), style.add_modifier(Modifier::BOLD)),
                    Span::styled(format!("{:<10}", if a.on { "turn on" } else { "turn off" }), style),
                    Span::styled(weeks_text(a.weeks), style),
                ])]
            });
        }
        if let Some(f) = &st.alarm_form {
            rows.push(blank());
            rows.push(heading(if f.index.is_some() { "Edit entry" } else { "New entry" }));
            rows.push(field(Ctl::AlarmTime, "Time", f.time.clone()));
            rows.push(choice(Ctl::AlarmAction, "Action", &["Turn on".into(), "Turn off".into()], Some(if f.on { 0 } else { 1 })));
            let list: Vec<Chip> = WEEKDAYS
                .iter()
                .map(|(n, bit)| Chip {
                    color: if f.weeks & bit != 0 { Some([0xD9, 0x77, 0x57]) } else { Some([0x87, 0x86, 0x7F]) },
                    text: n.to_string(),
                    title: if f.weeks & bit != 0 { "on".into() } else { "off".into() }
                })
                .collect();
            let mut days = chips(Ctl::AlarmDays, "Weekdays", &list, None, self.cursor_of(Ctl::AlarmDays));
            days.lines[0].spans.push(dim(format!("  ({})", weeks_text(f.weeks))));
            rows.push(days);
            rows.push(toggle(Ctl::AlarmActive, "Active", f.enable));
            rows.push(button(Ctl::AlarmSave, "Save and send", true));
            rows.push(button(Ctl::AlarmCancel, "Cancel", true));
            if f.index.is_some() {
                rows.push(button(Ctl::AlarmDelete, "Delete", true));
            }
        }
        rows.push(blank());
        rows.push(heading("Delay"));
        match (&st.store.delay, &st.delay_form) {
            (Some(d), None) => {
                let style = if d.enable { Style::default().add_modifier(Modifier::BOLD) } else { Style::default().fg(GRAY) };
                rows.push(Row {
                    ctl: None,
                    lines: vec![Line::from(vec![
                        Span::styled(format!("{:02}:{:02}  ", d.hour, d.minute), style),
                        dim(if d.enable { "countdown active" } else { "off" }),
                    ])]
                });
                rows.push(toggle(Ctl::DelayActive, "Active", d.enable));
                rows.push(button(Ctl::DelayEdit, "Edit", true));
                rows.push(button(Ctl::DelayDelete, "Delete", true));
            }
            (None, None) => {
                rows.push(button(Ctl::DelaySet, "Set", true));
                rows.push(note("A countdown after which the lamp runs its timer action (up to 23 h 59 min)."));
            }
            (_, Some(f)) => {
                rows.push(field(Ctl::DelayHour, "Hours", f.hour.to_string()));
                rows.push(field(Ctl::DelayMinute, "Minutes", f.minute.to_string()));
                rows.push(toggle(Ctl::DelayFormActive, "Active", f.enable));
                rows.push(button(Ctl::DelaySave, "Save and send", true));
                rows.push(button(Ctl::DelayCancel, "Cancel", true));
            }
        }
        rows.push(blank());
        rows.push(note("Schedules use the lamp clock, which is synced after connecting."));
        rows
    }

    fn settings_rows(&self, st: &State) -> Vec<Row> {
        let orders: Vec<String> = IC_ORDERS.iter().map(|s| s.to_string()).collect();
        let mut rows = vec![
            heading("Channel order"),
            note("If the strip shows wrong colours (for example red lights up green), pick the controller channel order."),
            choice(Ctl::Order, "Order", &orders, Some(st.store.settings.ic_order)),
            button(Ctl::OrderSend, "Send order", true),
        ];
        if st.is_magic() {
            rows.push(blank());
            rows.push(heading("LED count (IC)"));
            rows.push(field(Ctl::IcLength, "LEDs (16–2048)", st.store.settings.ic_length.to_string()));
            rows.push(button(Ctl::IcLengthSend, "Send", true));
        }
        rows.push(blank());
        rows.push(heading("Time sync"));
        rows.push(note("The time is sent automatically after connecting. Schedules use the lamp clock."));
        let connected: Vec<_> = st.devices.iter().enumerate().filter(|(_, d)| d.connected).collect();
        if connected.is_empty() {
            rows.push(note("no connected lamps"));
        }
        for (i, d) in connected {
            rows.push(button(Ctl::SyncTime(i), &format!("Sync: {}", d.name), true));
        }
        rows.push(blank());
        rows.push(heading("Sent packets"));
        if st.log.is_empty() {
            rows.push(note("// Bytes sent to characteristic 0xAE01 appear here."));
        }
        for l in &st.log {
            rows.push(Row {
                ctl: None,
                lines: vec![Line::from(vec![dim(format!("{}  ", l.stamp)), Span::styled(l.text.clone(), Style::default().fg(if l.dim { GRAY } else { OLIVE }))])]
            });
        }
        rows.push(blank());
        rows.push(note(format!("State file: {}", st.state_path().display())));
        rows
    }

    fn favorite_rows(&self, st: &State) -> Vec<Row> {
        let mut rows = vec![heading("Favourites")];
        if st.store.favorites.is_empty() {
            rows.push(note("No favourites yet. Use ☆ Add to favourites on the Colour, Segments or Modes screen."));
        }
        for (i, f) in st.store.favorites.iter().enumerate() {
            let mut first = match &f.kind {
                FavoriteKind::Color { swatch, .. } => vec![Span::styled("    ", Style::default().bg(rgb(*swatch))), Span::raw(" ")],
                FavoriteKind::Mode { dots: d, .. } | FavoriteKind::Segments { dots: d, .. } => dots(d)
            };
            first.push(Span::styled(f.name.clone(), Style::default().add_modifier(Modifier::BOLD)));
            rows.push(Row { ctl: Some(Ctl::Favorite(i)), lines: vec![Line::from(first), Line::from(vec![Span::raw("    "), dim(st.favorite_details(f))])] });
        }
        rows
    }

    fn setup_rows(&self, st: &State) -> Vec<Row> {
        let mut rows = vec![heading("Setups")];
        if st.store.favorites.is_empty() || st.devices.len() < 2 {
            rows.push(note("Setups apply a favourite to each lamp with one action. They need at least two lamps and one favourite."));
            return rows;
        }
        if st.setup_form.is_none() {
            rows.push(button(Ctl::NewSetup, "+ New setup", true));
        }
        if st.store.setups.is_empty() && st.setup_form.is_none() {
            rows.push(note("A favourite for each lamp, applied with one click. Apply favourites to the lamps first, then the new setup starts from them."));
        }
        for (i, s) in st.store.setups.iter().enumerate() {
            rows.push(Row {
                ctl: Some(Ctl::Setup(i)),
                lines: vec![Line::from(Span::styled(s.name.clone(), Style::default().add_modifier(Modifier::BOLD))), Line::from(vec![Span::raw("    "), dim(st.setup_details(s))])]
            });
        }
        if let Some(form) = &st.setup_form {
            rows.push(blank());
            rows.push(field(Ctl::SetupName, "Setup name", form.name.clone()));
            for (i, (dev_id, action)) in form.lamps.iter().enumerate() {
                let name = st.dev(dev_id).map(|d| d.name.clone()).unwrap_or_else(|| "unknown lamp".into());
                let value = match action.as_str() {
                    "" => "leave unchanged".to_string(),
                    "off" => "turn off".to_string(),
                    id => st.store.favorites.iter().find(|f| f.id == id).map(|f| f.name.clone()).unwrap_or_else(|| "?".into())
                };
                rows.push(spinner(Ctl::SetupLamp(i), &name, value));
            }
            rows.push(button(Ctl::SetupSave, "Save setup", true));
            rows.push(button(Ctl::SetupCancel, "Cancel", true));
        }
        rows
    }

    // ---------- drawing ----------
    fn draw(&mut self, f: &mut Frame, core: &Shared) {
        let area = f.area();
        let [header, tabs, body, footer] =
            Layout::vertical([Constraint::Length(2), Constraint::Length(1), Constraint::Min(5), Constraint::Length(2)]).areas(area);
        let st = core.lock();

        let selected: Vec<String> = st.selected_devices().iter().map(|d| d.name.clone()).collect();
        let power = if st.power {
            Span::styled(" ⏻ ON ", Style::default().fg(IVORY).bg(CLAY))
        } else {
            Span::styled(" ⏻ OFF ", Style::default().fg(GRAY).add_modifier(Modifier::REVERSED))
        };
        let head = vec![
            Line::from(vec![
                Span::styled("Raingel · lamp control  ", Style::default().add_modifier(Modifier::BOLD)),
                power,
                Span::raw("  "),
                Span::styled(format!("{} connected", st.connected_count()), Style::default().fg(if st.connected_count() > 0 { OLIVE } else { GRAY })),
            ]),
            Line::from(dim(format!(
                "controlling: {}  ·  type: {}",
                if selected.is_empty() { "nothing selected".to_string() } else { selected.join(", ") },
                group_name(st.group())
            ))),
        ];
        f.render_widget(Paragraph::new(head), header);

        let titles: Vec<String> = TABS.iter().enumerate().map(|(i, (_, n))| format!("{} {n}", i + 1)).collect();
        let current = TABS.iter().position(|(t, _)| *t == st.tab).unwrap_or(0);
        f.render_widget(
            Tabs::new(titles).select(current).highlight_style(Style::default().fg(CLAY).add_modifier(Modifier::BOLD)).divider("│"),
            tabs
        );

        let block = Block::default().borders(Borders::TOP).border_style(Style::default().fg(GRAY));
        let inner = block.inner(body);
        f.render_widget(block, body);

        let show_wheel = st.tab == MainTab::Color && st.color_tab == ColorTab::Color && inner.width >= 100;
        let (content, wheel_area) = if show_wheel {
            let [c, w] = Layout::horizontal([Constraint::Min(60), Constraint::Length(WHEEL_W + 2)]).areas(inner);
            (c, Some(Rect { x: w.x + 1, y: w.y + 1, width: WHEEL_W, height: WHEEL_H.min(w.height.saturating_sub(1)) }))
        } else {
            (inner, None)
        };

        let rows = self.rows(&st, content.width as usize);
        self.fix_focus(&rows);
        let mut lines: Vec<Line> = Vec::new();
        let mut focus_line = (0, 0);
        for row in &rows {
            let focused = row.ctl.is_some() && row.ctl == self.focus;
            if focused {
                focus_line = (lines.len(), row.lines.len());
            }
            for (k, l) in row.lines.iter().enumerate() {
                let mut l = l.clone();
                let prefix = if focused && k == 0 {
                    Span::styled("› ", Style::default().fg(CLAY).add_modifier(Modifier::BOLD))
                } else {
                    Span::raw("  ")
                };
                l.spans.insert(0, prefix);
                if focused && row.ctl != Some(Ctl::ModeGrid) {
                    l = l.style(Style::default().bg(Color::Rgb(0x3D, 0x3D, 0x3A)));
                }
                lines.push(l);
            }
        }
        let height = content.height as usize;
        if focus_line.0 < self.scroll {
            self.scroll = focus_line.0;
        } else if focus_line.0 + focus_line.1 > self.scroll + height {
            self.scroll = (focus_line.0 + focus_line.1).saturating_sub(height);
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(1));
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.scroll as u16, 0)), content);

        self.wheel = wheel_area;
        if let Some(w) = wheel_area {
            f.render_widget(Wheel { hue: st.hue, sat: st.sat, rgb: st.rgb }, w);
        }

        let hint = self.hint(&st);
        let last = st.log.front().map(|l| format!("last packet {}  {}", l.stamp, l.text)).unwrap_or_default();
        let toast = st.toast_text().map(|t| t.to_string());
        let foot = vec![
            match toast {
                Some(t) => Line::from(Span::styled(format!(" {t} "), Style::default().fg(IVORY).bg(Color::Rgb(0x14, 0x14, 0x13)))),
                None => Line::from(dim(hint))
            },
            Line::from(vec![dim("q quit · 1-8/Tab screens · ↑↓ move · p power · ? help   "), Span::styled(last, Style::default().fg(OLIVE))]),
        ];
        f.render_widget(Paragraph::new(foot), footer);

        if let Some(cursor) = self.chooser {
            self.draw_chooser(f, &st, cursor);
        }
        if let Some(p) = &self.prompt {
            let w = 60.min(area.width.saturating_sub(4));
            let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height / 2).saturating_sub(2), width: w, height: 3.min(area.height) };
            f.render_widget(Clear, r);
            f.render_widget(
                Paragraph::new(format!("{}▏", p.value)).block(Block::bordered().title(format!(" {} · Enter save · Esc cancel ", p.title)).border_style(Style::default().fg(CLAY))),
                r
            );
        }
        if self.help {
            draw_help(f, area);
        }
    }

    fn draw_chooser(&self, f: &mut Frame, st: &State, cursor: usize) {
        let area = f.area();
        let w = 76.min(area.width.saturating_sub(4));
        let h = 16.min(area.height.saturating_sub(2));
        let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h };
        f.render_widget(Clear, r);
        let list = chooser_list(st);
        let mut lines = vec![Line::from(dim("Scanning for lamps (service 0xAF30 or manufacturer id 22872)…")), Line::raw("")];
        if list.is_empty() {
            lines.push(Line::from(dim("Nothing found yet. Turn the lamp on and keep it close.")));
        }
        for (i, a) in list.iter().enumerate() {
            let group = a.manufacturer.as_ref().filter(|m| m.len() >= 2).map(|m| group_name(m[0])).unwrap_or_else(|| "type unknown".into());
            let text = format!(
                "{:<22} {:<20} {:>5}  {}",
                a.name.clone().unwrap_or_else(|| "Lamp".into()),
                a.id,
                a.rssi.map(|r| format!("{r}dBm")).unwrap_or_default(),
                group
            );
            let style = if i == cursor { Style::default().fg(Color::Black).bg(IVORY) } else { Style::default() };
            lines.push(Line::from(Span::styled(text, style)));
        }
        f.render_widget(
            Paragraph::new(lines).block(Block::bordered().title(" Add lamp · Enter connect · Esc close ").border_style(Style::default().fg(CLAY))),
            r
        );
    }

    fn hint(&self, st: &State) -> String {
        let Some(ctl) = self.focus else { return String::new() };
        match ctl {
            Ctl::Device(_) => "Space control this lamp · Enter connect/disconnect · ←→ lamp type · r rename · x remove · a add lamp".into(),
            Ctl::Hue | Ctl::Sat | Ctl::Cct | Ctl::Wb | Ctl::MotorSpeed | Ctl::LaserSpeed | Ctl::Brightness | Ctl::Speed | Ctl::ModeBrightness | Ctl::Sensitivity => {
                "←→ ±1 · Shift+←→ or [ ] ±10 · Home/End · Enter type a value".into()
            }
            Ctl::Presets | Ctl::CctPresets | Ctl::WbPresets | Ctl::SegColor(_) | Ctl::MicColor => "←→ choose · Enter/Space pick".into(),
            Ctl::ModeGrid => "←→↑↓ choose · Enter/Space start the effect".into(),
            Ctl::AlarmDays => "←→ choose a day · Space toggle".into(),
            Ctl::Alarm(_) => "Space active on/off · Enter edit".into(),
            Ctl::Favorite(_) => format!(
                "Enter apply to {} · r rename · x remove",
                if st.selected_devices().is_empty() { "selected lamps".to_string() } else { st.selected_devices().iter().map(|d| d.name.clone()).collect::<Vec<_>>().join(", ") }
            ),
            Ctl::Setup(_) => "Enter apply · r rename · x remove".into(),
            Ctl::SegEnd(_) | Ctl::SegLength | Ctl::IcLength | Ctl::Hex | Ctl::AlarmTime | Ctl::SetupName => "Enter edit · ←→ adjust numbers".into(),
            Ctl::DelayHour | Ctl::DelayMinute => "←→ adjust · Enter type a value".into(),
            Ctl::ColorTab | Ctl::Scene | Ctl::MicSource | Ctl::Effect | Ctl::AlarmAction | Ctl::Order | Ctl::SetupLamp(_) => "←→ change".into(),
            _ => "Enter/Space activate".into()
        }
    }

    fn fix_focus(&mut self, rows: &[Row]) {
        let ctls: Vec<Ctl> = rows.iter().filter_map(|r| r.ctl).collect();
        if ctls.is_empty() {
            self.focus = None;
            return;
        }
        match self.focus.and_then(|f| ctls.iter().position(|c| *c == f)) {
            Some(i) => self.focus_idx = i,
            None => {
                self.focus_idx = self.focus_idx.min(ctls.len() - 1);
                self.focus = Some(ctls[self.focus_idx]);
            }
        }
    }

    fn focusables(&mut self, core: &Shared) -> Vec<Ctl> {
        let st = core.lock();
        let rows = self.rows(&st, self.grid_width());
        drop(st);
        self.fix_focus(&rows);
        rows.iter().filter_map(|r| r.ctl).collect()
    }

    fn grid_width(&self) -> usize {
        self.grid_cols * 24 + 2
    }

    fn move_focus(&mut self, core: &Shared, delta: i64) {
        let ctls = self.focusables(core);
        if ctls.is_empty() {
            return;
        }
        let i = (self.focus_idx as i64 + delta).clamp(0, ctls.len() as i64 - 1) as usize;
        self.focus_idx = i;
        self.focus = Some(ctls[i]);
    }

    // ---------- input ----------
    fn on_click(&mut self, core: &Shared, col: u16, row: u16) {
        let Some(w) = self.wheel else { return };
        if col < w.x || col >= w.x + w.width || row < w.y || row >= w.y + w.height {
            return;
        }
        let c = w.width as f64 / 2.0;
        let px = (col - w.x) as f64 + 0.5;
        let py = ((row - w.y) as f64) * 2.0 + 1.0;
        let dx = px - c;
        let dy = c - py;
        let mut h = dy.atan2(dx).to_degrees();
        if h < 0.0 {
            h += 360.0;
        }
        let s = ((dx * dx + dy * dy).sqrt() / c).min(1.0);
        core.pick_hue_sat(h, s);
    }

    fn on_key(&mut self, core: &Shared, key: KeyEvent) -> Flow {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Flow::Quit;
        }
        if self.help {
            self.help = false;
            return Flow::Continue;
        }
        if self.prompt.is_some() {
            self.prompt_key(core, key);
            return Flow::Continue;
        }
        if let Some(cursor) = self.chooser {
            self.chooser_key(core, key, cursor);
            return Flow::Continue;
        }
        match key.code {
            KeyCode::Char('q') => return Flow::Quit,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('p') => core.toggle_power(),
            KeyCode::Char(c @ '1'..='8') => self.set_tab(core, TABS[c as usize - '1' as usize].0),
            KeyCode::Tab => {
                let i = TABS.iter().position(|(t, _)| *t == self.last_tab).unwrap_or(0);
                self.set_tab(core, TABS[(i + 1) % TABS.len()].0);
            }
            KeyCode::BackTab => {
                let i = TABS.iter().position(|(t, _)| *t == self.last_tab).unwrap_or(0);
                self.set_tab(core, TABS[(i + TABS.len() - 1) % TABS.len()].0);
            }
            KeyCode::Char('a') if self.last_tab == MainTab::Devices => self.open_chooser(core),
            KeyCode::PageDown => self.move_focus(core, 5),
            KeyCode::PageUp => self.move_focus(core, -5),
            _ => {
                let ctls = self.focusables(core);
                let Some(ctl) = self.focus else { return Flow::Continue };
                if !self.control_key(core, ctl, key) {
                    match key.code {
                        KeyCode::Down | KeyCode::Char('j') => self.move_focus(core, 1),
                        KeyCode::Up | KeyCode::Char('k') => self.move_focus(core, -1),
                        _ => {}
                    }
                }
                let _ = ctls;
            }
        }
        Flow::Continue
    }

    fn open_chooser(&mut self, core: &Shared) {
        core.set_choosing(true);
        if !core.ble.available() {
            let mut st = core.lock();
            let e = st.ble_error.clone().unwrap_or_else(|| "No Bluetooth adapter found.".into());
            st.toast_for(e, 8000);
        }
        self.chooser = Some(0);
    }

    fn chooser_key(&mut self, core: &Shared, key: KeyEvent, cursor: usize) {
        let list = chooser_list(&core.lock());
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.chooser = None;
                core.set_choosing(false);
            }
            KeyCode::Down | KeyCode::Char('j') => self.chooser = Some((cursor + 1).min(list.len().saturating_sub(1))),
            KeyCode::Up | KeyCode::Char('k') => self.chooser = Some(cursor.saturating_sub(1)),
            KeyCode::Enter => {
                if let Some(a) = list.get(cursor) {
                    self.chooser = None;
                    core.set_choosing(false);
                    let c = core.clone();
                    let id = a.id.clone();
                    let name = a.name.clone();
                    tokio::spawn(async move { c.add_device(&id, name).await });
                }
            }
            _ => {}
        }
    }

    fn open_prompt(&mut self, title: &str, value: String, target: PromptTarget) {
        self.prompt = Some(Prompt { title: title.to_string(), value, target });
    }

    fn prompt_key(&mut self, core: &Shared, key: KeyEvent) {
        let Some(p) = self.prompt.as_mut() else { return };
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Backspace => {
                p.value.pop();
            }
            KeyCode::Char(c) => p.value.push(c),
            KeyCode::Enter => {
                let p = self.prompt.take().unwrap();
                self.commit_prompt(core, p);
            }
            _ => {}
        }
    }

    fn commit_prompt(&mut self, core: &Shared, p: Prompt) {
        let value = p.value.trim().to_string();
        let number = value.parse::<i64>().ok();
        match p.target {
            PromptTarget::RenameDevice(id) => core.lock().rename_device(&id, &value),
            PromptTarget::RenameFavorite(id) => core.lock().rename_favorite(&id, &p.value),
            PromptTarget::RenameSetup(id) => core.lock().rename_setup(&id, &p.value),
            PromptTarget::Hex => match parse_hex(&value) {
                Some(c) => {
                    core.lock().preset_index = None;
                    core.set_rgb(c);
                }
                None => core.lock().toast("Enter a colour as #rrggbb.")
            },
            PromptTarget::Slider(ctl) => match number {
                Some(n) => self.set_slider(core, ctl, n),
                None => core.lock().toast("Enter a number.")
            },
            PromptTarget::SegLength => {
                let mut st = core.lock();
                st.store.settings.ic_length = number.unwrap_or(0);
                st.split_segments_evenly();
            }
            PromptTarget::SegEnd(i) => {
                let mut st = core.lock();
                if let Some(seg) = st.store.segments.get_mut(i) {
                    seg.end = number;
                }
                st.persist();
            }
            PromptTarget::IcLength => core.lock().store.settings.ic_length = number.unwrap_or(0),
            PromptTarget::AlarmTime => {
                if let Some(f) = core.lock().alarm_form.as_mut() {
                    f.time = value;
                }
            }
            PromptTarget::DelayHour => {
                if let Some(f) = core.lock().delay_form.as_mut() {
                    f.hour = number.unwrap_or(0);
                }
            }
            PromptTarget::DelayMinute => {
                if let Some(f) = core.lock().delay_form.as_mut() {
                    f.minute = number.unwrap_or(0);
                }
            }
            PromptTarget::SetupName => {
                if let Some(f) = core.lock().setup_form.as_mut() {
                    f.name = p.value.clone();
                }
            }
        }
    }

    fn slider_value(&self, st: &State, ctl: Ctl) -> Option<(i64, i64, i64)> {
        Some(match ctl {
            Ctl::Hue => (js_round(st.hue) as i64, 0, 359),
            Ctl::Sat => (js_round(st.sat * 100.0) as i64, 0, 100),
            Ctl::Cct => (st.cct as i64, 0, 255),
            Ctl::Wb => (st.wb as i64, 0, 255),
            Ctl::MotorSpeed => (st.motor_speed as i64, 0, 100),
            Ctl::LaserSpeed => (st.laser_speed as i64, 0, 100),
            Ctl::Brightness => (st.store.settings.brightness as i64, 0, 100),
            Ctl::Speed => (st.speed as i64, 0, 100),
            Ctl::ModeBrightness => (st.mode_brightness as i64, 0, 100),
            Ctl::Sensitivity => (st.sensitivity as i64, 0, 100),
            _ => return None
        })
    }

    fn set_slider(&mut self, core: &Shared, ctl: Ctl, v: i64) {
        let Some((_, min, max)) = self.slider_value(&core.lock(), ctl) else { return };
        let v = v.clamp(min, max);
        match ctl {
            Ctl::Hue => {
                let sat = core.lock().sat;
                core.pick_hue_sat(v as f64, sat);
            }
            Ctl::Sat => {
                let hue = core.lock().hue;
                core.pick_hue_sat(hue, v as f64 / 100.0);
            }
            Ctl::Cct => core.set_cct(v as u8),
            Ctl::Wb => core.set_wb(v as u8),
            Ctl::MotorSpeed => {
                {
                    let mut st = core.lock();
                    st.motor_speed = v as u8;
                    st.motor_on = true;
                }
                core.set_motor();
            }
            Ctl::LaserSpeed => {
                {
                    let mut st = core.lock();
                    st.laser_speed = v as u8;
                    st.laser_on = true;
                }
                core.set_laser();
            }
            Ctl::Brightness => core.set_brightness(v as u8),
            Ctl::Speed => {
                core.lock().speed = v as u8;
                core.send_mode();
            }
            Ctl::ModeBrightness => {
                core.lock().mode_brightness = v as u8;
                core.send_mode();
            }
            Ctl::Sensitivity => {
                core.lock().sensitivity = v as u8;
                core.send_device_mic();
            }
            _ => {}
        }
    }

    // Moves the cursor of a chip row or the mode grid; returns false at the edges of the grid.
    fn move_cursor(&mut self, ctl: Ctl, len: usize, initial: usize, delta: i64) -> bool {
        if self.cursor_for != Some(ctl) {
            self.cursor_for = Some(ctl);
            self.cursor = initial.min(len.saturating_sub(1));
            if delta.abs() == 1 && ctl != Ctl::ModeGrid {
                return true;
            }
        }
        let next = self.cursor as i64 + delta;
        if next < 0 || next >= len as i64 {
            return false;
        }
        self.cursor = next as usize;
        true
    }

    fn chips_key(&mut self, ctl: Ctl, key: KeyEvent, len: usize, initial: usize) -> Option<usize> {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                self.move_cursor(ctl, len, initial, -1);
                None
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.move_cursor(ctl, len, initial, 1);
                None
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if self.cursor_for != Some(ctl) {
                    self.cursor_for = Some(ctl);
                    self.cursor = initial.min(len.saturating_sub(1));
                }
                Some(self.cursor)
            }
            _ => None
        }
    }

    // Returns true when the key was handled by the focused control.
    fn control_key(&mut self, core: &Shared, ctl: Ctl, key: KeyEvent) -> bool {
        let activate = matches!(key.code, KeyCode::Enter | KeyCode::Char(' '));
        let left = matches!(key.code, KeyCode::Left | KeyCode::Char('h'));
        let right = matches!(key.code, KeyCode::Right | KeyCode::Char('l'));
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        let slider = self.slider_value(&core.lock(), ctl);
        if let Some((value, min, max)) = slider {
            let next = match key.code {
                KeyCode::Left | KeyCode::Char('h') => Some(value - if shift { 10 } else { 1 }),
                KeyCode::Right | KeyCode::Char('l') => Some(value + if shift { 10 } else { 1 }),
                KeyCode::Char('[') => Some(value - 10),
                KeyCode::Char(']') => Some(value + 10),
                KeyCode::Home => Some(min),
                KeyCode::End => Some(max),
                KeyCode::Enter => {
                    self.open_prompt("Value", value.to_string(), PromptTarget::Slider(ctl));
                    return true;
                }
                _ => None
            };
            if let Some(n) = next {
                let n = if ctl == Ctl::Hue { n.rem_euclid(360) } else { n.clamp(min, max) };
                self.set_slider(core, ctl, n);
                return true;
            }
            return false;
        }

        match ctl {
            Ctl::AddLamp if activate => self.open_chooser(core),
            Ctl::Device(i) => {
                let Some(d) = core.lock().devices.get(i).cloned() else { return false };
                match key.code {
                    KeyCode::Char(' ') => core.lock().toggle_select(&d.id),
                    KeyCode::Enter => {
                        if d.connected {
                            core.disconnect(&d.id);
                        } else if !d.connecting {
                            let c = core.clone();
                            tokio::spawn(async move {
                                c.connect(&d.id, false).await;
                            });
                        }
                    }
                    KeyCode::Char('r') => self.open_prompt("Rename lamp", d.name.clone(), PromptTarget::RenameDevice(d.id.clone())),
                    KeyCode::Char('x') | KeyCode::Delete => core.forget(&d.id),
                    KeyCode::Left | KeyCode::Right => {
                        // unknown + GROUPS, like the select in the browser
                        let mut options: Vec<Option<u8>> = vec![None];
                        options.extend(GROUPS.iter().map(|(id, _)| Some(*id)));
                        let pos = options.iter().position(|g| *g == d.group).unwrap_or(0) as i64;
                        let next = (pos + if right { 1 } else { -1 }).rem_euclid(options.len() as i64) as usize;
                        core.lock().set_group(&d.id, options[next]);
                    }
                    _ => return false
                }
            }

            Ctl::ColorTab if left || right => {
                let mut st = core.lock();
                let tabs = st.color_tabs();
                let pos = tabs.iter().position(|t| *t == st.color_tab).unwrap_or(0) as i64;
                st.color_tab = tabs[(pos + if right { 1 } else { -1 }).rem_euclid(tabs.len() as i64) as usize];
            }
            Ctl::Presets => {
                let initial = core.lock().preset_index.and_then(|p| PALETTE_ORDER.iter().position(|x| *x == p)).unwrap_or(0);
                match self.chips_key(ctl, key, PALETTE_ORDER.len(), initial) {
                    Some(c) => core.pick_preset(PALETTE_ORDER[c]),
                    None => return left || right
                }
            }
            Ctl::Hex if activate => {
                let c = core.lock().rgb;
                self.open_prompt("Colour #rrggbb", format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]), PromptTarget::Hex);
            }
            Ctl::CctPresets => {
                let presets = cct_presets();
                let initial = presets.iter().position(|v| *v == core.lock().cct).unwrap_or(0);
                match self.chips_key(ctl, key, presets.len(), initial) {
                    Some(c) => core.set_cct(presets[c]),
                    None => return left || right
                }
            }
            Ctl::WbPresets => {
                let presets = wb_presets();
                let initial = presets.iter().position(|v| *v == core.lock().wb).unwrap_or(0);
                match self.chips_key(ctl, key, presets.len(), initial) {
                    Some(c) => core.set_wb(presets[c]),
                    None => return left || right
                }
            }
            Ctl::MotorOn if activate => {
                {
                    let mut st = core.lock();
                    st.motor_on = !st.motor_on;
                }
                core.set_motor();
            }
            Ctl::LaserOn if activate => {
                {
                    let mut st = core.lock();
                    st.laser_on = !st.laser_on;
                }
                core.set_laser();
            }
            Ctl::SegLength => match key.code {
                KeyCode::Enter => {
                    let v = core.lock().store.settings.ic_length;
                    self.open_prompt("Full LED count of the strip", v.to_string(), PromptTarget::SegLength);
                }
                KeyCode::Left | KeyCode::Right => {
                    let mut st = core.lock();
                    st.store.settings.ic_length = (st.store.settings.ic_length + if right { 1 } else { -1 }).clamp(MIN_LEDS, MAX_LEDS);
                    st.split_segments_evenly();
                }
                _ => return false
            },
            Ctl::SegColor(i) => {
                let current = core.lock().store.segments.get(i).map(|s| s.color).unwrap_or(0);
                let initial = PALETTE_ORDER.iter().position(|c| *c == current).unwrap_or(0);
                match self.chips_key(ctl, key, PALETTE_ORDER.len(), initial) {
                    Some(c) => {
                        let mut st = core.lock();
                        if let Some(seg) = st.store.segments.get_mut(i) {
                            seg.color = PALETTE_ORDER[c];
                        }
                        st.persist();
                    }
                    None => return left || right
                }
            }
            Ctl::SegEnd(i) => match key.code {
                KeyCode::Enter => {
                    let v = core.lock().store.segments.get(i).and_then(|s| s.end).map(|e| e.to_string()).unwrap_or_default();
                    self.open_prompt("Up to LED (counted from the bottom)", v, PromptTarget::SegEnd(i));
                }
                KeyCode::Left | KeyCode::Right => {
                    let mut st = core.lock();
                    let step = if shift { 10 } else { 1 };
                    if let Some(seg) = st.store.segments.get_mut(i) {
                        seg.end = Some((seg.end.unwrap_or(MIN_LEDS) + if right { step } else { -step }).max(0));
                    }
                    st.persist();
                }
                _ => return false
            },
            Ctl::SegApply(i) if activate => {
                let (busy, err) = {
                    let st = core.lock();
                    (st.segments_busy, st.segments_error())
                };
                if !busy && err.is_none() {
                    let c = core.clone();
                    tokio::spawn(async move { c.apply_segment(i).await });
                }
            }
            Ctl::SegRemove(i) if activate => core.lock().remove_segment(i),
            Ctl::SegApplyAll if activate => {
                let c = core.clone();
                tokio::spawn(async move { c.apply_segments().await });
            }
            Ctl::SegAdd if activate => core.lock().add_segment(),
            Ctl::SegSplit if activate => core.lock().split_segments_evenly(),
            Ctl::SegFav if activate => core.lock().add_segments_favorite(),
            Ctl::SegRestore if activate => core.restore_length(),
            Ctl::ColorFav if activate => core.lock().add_color_favorite(),

            Ctl::Scene if left || right => {
                let (n, cur) = {
                    let st = core.lock();
                    (st.scene_tabs().len(), st.scene_tab)
                };
                if n > 0 {
                    core.pick_scene((cur as i64 + if right { 1 } else { -1 }).rem_euclid(n as i64) as usize);
                    self.cursor_for = None;
                }
            }
            Ctl::ModeGrid => {
                let (len, initial) = {
                    let st = core.lock();
                    (st.mode_items().len(), st.mode_index.unwrap_or(0))
                };
                let cols = self.grid_cols.max(1) as i64;
                let moved = match key.code {
                    KeyCode::Left | KeyCode::Char('h') => self.move_cursor(ctl, len, initial, -1),
                    KeyCode::Right | KeyCode::Char('l') => self.move_cursor(ctl, len, initial, 1),
                    KeyCode::Up | KeyCode::Char('k') => self.move_cursor(ctl, len, initial, -cols),
                    KeyCode::Down | KeyCode::Char('j') => self.move_cursor(ctl, len, initial, cols),
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if self.cursor_for != Some(ctl) {
                            self.cursor_for = Some(ctl);
                            self.cursor = initial;
                        }
                        if self.cursor < len {
                            core.pick_mode(self.cursor);
                        }
                        true
                    }
                    _ => false
                };
                if !moved && matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k')) {
                    self.cursor_for = None;
                }
                return moved;
            }
            Ctl::ModeFav if activate => core.lock().add_mode_favorite(),

            Ctl::MicSource if left || right || activate => {
                let src = if core.lock().mic_source == MicSource::Device { MicSource::Phone } else { MicSource::Device };
                core.set_mic_source(src);
            }
            Ctl::Effect if left || right => {
                let cur = core.lock().mic_effect as i64;
                core.pick_effect((cur + if right { 1 } else { -1 }).rem_euclid(4) as u8);
            }
            Ctl::Listen if activate => {
                if core.lock().recording {
                    core.stop_phone_mic();
                } else {
                    core.start_phone_mic();
                }
            }
            Ctl::MicColor => {
                let current = core.lock().store.settings.mic_color;
                let initial = if current < 0 { 0 } else { PALETTE_ORDER.iter().position(|c| *c as i32 == current).map(|p| p + 1).unwrap_or(0) };
                match self.chips_key(ctl, key, PALETTE_ORDER.len() + 1, initial) {
                    Some(0) => core.set_mic_color(-1),
                    Some(c) => core.set_mic_color(PALETTE_ORDER[c - 1] as i32),
                    None => return left || right
                }
            }

            Ctl::AlarmAdd if activate => core.lock().new_alarm(),
            Ctl::Alarm(i) => match key.code {
                KeyCode::Char(' ') => core.toggle_alarm(i),
                KeyCode::Enter => {
                    core.lock().edit_alarm(i);
                    self.focus = Some(Ctl::AlarmTime);
                }
                _ => return false
            },
            Ctl::AlarmTime => match key.code {
                KeyCode::Enter => {
                    let t = core.lock().alarm_form.as_ref().map(|f| f.time.clone()).unwrap_or_default();
                    self.open_prompt("Time HH:MM", t, PromptTarget::AlarmTime);
                }
                KeyCode::Left | KeyCode::Right => {
                    let mut st = core.lock();
                    if let Some(f) = st.alarm_form.as_mut() {
                        shift_time(f, if shift { 60 } else { 1 } * if right { 1 } else { -1 });
                    }
                }
                _ => return false
            },
            Ctl::AlarmAction if left || right || activate => {
                if let Some(f) = core.lock().alarm_form.as_mut() {
                    f.on = !f.on;
                }
            }
            Ctl::AlarmDays => {
                match self.chips_key(ctl, key, 7, 0) {
                    Some(d) => {
                        if let Some(f) = core.lock().alarm_form.as_mut() {
                            f.weeks ^= WEEKDAYS[d].1;
                        }
                    }
                    None => return left || right
                }
            }
            Ctl::AlarmActive if activate => {
                if let Some(f) = core.lock().alarm_form.as_mut() {
                    f.enable = !f.enable;
                }
            }
            Ctl::AlarmSave if activate => core.save_alarm(),
            Ctl::AlarmCancel if activate => core.lock().alarm_form = None,
            Ctl::AlarmDelete if activate => {
                let index = core.lock().alarm_form.as_ref().and_then(|f| f.index);
                if let Some(i) = index {
                    core.remove_alarm(i);
                }
            }
            Ctl::DelaySet | Ctl::DelayEdit if activate => core.lock().open_delay(),
            Ctl::DelayActive if activate => core.toggle_delay(),
            Ctl::DelayDelete if activate => core.remove_delay(),
            Ctl::DelayHour | Ctl::DelayMinute => match key.code {
                KeyCode::Enter => {
                    let (h, m) = core.lock().delay_form.as_ref().map(|f| (f.hour, f.minute)).unwrap_or((0, 0));
                    if ctl == Ctl::DelayHour {
                        self.open_prompt("Hours (0-23)", h.to_string(), PromptTarget::DelayHour);
                    } else {
                        self.open_prompt("Minutes (0-59)", m.to_string(), PromptTarget::DelayMinute);
                    }
                }
                KeyCode::Left | KeyCode::Right => {
                    if let Some(f) = core.lock().delay_form.as_mut() {
                        let d = if right { 1 } else { -1 };
                        if ctl == Ctl::DelayHour {
                            f.hour = (f.hour + d).clamp(0, 23);
                        } else {
                            f.minute = (f.minute + d).clamp(0, 59);
                        }
                    }
                }
                _ => return false
            },
            Ctl::DelayFormActive if activate => {
                if let Some(f) = core.lock().delay_form.as_mut() {
                    f.enable = !f.enable;
                }
            }
            Ctl::DelaySave if activate => core.save_delay(),
            Ctl::DelayCancel if activate => core.lock().delay_form = None,

            Ctl::Order if left || right => {
                let mut st = core.lock();
                let cur = st.store.settings.ic_order as i64;
                st.store.settings.ic_order = (cur + if right { 1 } else { -1 }).rem_euclid(IC_ORDERS.len() as i64) as usize;
            }
            Ctl::OrderSend if activate => core.send_ic_order(),
            Ctl::IcLength => match key.code {
                KeyCode::Enter => {
                    let v = core.lock().store.settings.ic_length;
                    self.open_prompt("LED count (16–2048)", v.to_string(), PromptTarget::IcLength);
                }
                KeyCode::Left | KeyCode::Right => {
                    let mut st = core.lock();
                    let step = if shift { 10 } else { 1 };
                    st.store.settings.ic_length = (st.store.settings.ic_length + if right { step } else { -step }).clamp(MIN_LEDS, MAX_LEDS);
                }
                _ => return false
            },
            Ctl::IcLengthSend if activate => core.send_ic_length(),
            Ctl::SyncTime(i) if activate => {
                let id = core.lock().devices.get(i).map(|d| d.id.clone());
                if let Some(id) = id {
                    tokio::spawn(core.sync_time(&id));
                }
            }

            Ctl::Favorite(i) => {
                let Some(f) = core.lock().store.favorites.get(i).cloned() else { return false };
                match key.code {
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let c = core.clone();
                        tokio::spawn(async move { c.apply_favorite(&f).await });
                    }
                    KeyCode::Char('r') => self.open_prompt("Rename favourite", f.name.clone(), PromptTarget::RenameFavorite(f.id.clone())),
                    KeyCode::Char('x') | KeyCode::Delete => core.lock().remove_favorite(&f.id),
                    _ => return false
                }
            }

            Ctl::NewSetup if activate => {
                core.lock().new_setup();
                self.focus = Some(Ctl::SetupName);
            }
            Ctl::Setup(i) => {
                let Some(s) = core.lock().store.setups.get(i).cloned() else { return false };
                match key.code {
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        let c = core.clone();
                        tokio::spawn(async move { c.apply_setup(&s).await });
                    }
                    KeyCode::Char('r') => self.open_prompt("Rename setup", s.name.clone(), PromptTarget::RenameSetup(s.id.clone())),
                    KeyCode::Char('x') | KeyCode::Delete => core.lock().remove_setup(&s.id),
                    _ => return false
                }
            }
            Ctl::SetupName if activate => {
                let name = core.lock().setup_form.as_ref().map(|f| f.name.clone()).unwrap_or_default();
                self.open_prompt("Setup name", name, PromptTarget::SetupName);
            }
            Ctl::SetupLamp(i) if left || right => {
                let mut st = core.lock();
                let mut options = vec![String::new(), "off".to_string()];
                options.extend(st.store.favorites.iter().map(|f| f.id.clone()));
                if let Some(form) = st.setup_form.as_mut()
                    && let Some(lamp) = form.lamps.get_mut(i)
                {
                    let pos = options.iter().position(|o| *o == lamp.1).unwrap_or(0) as i64;
                    lamp.1 = options[(pos + if right { 1 } else { -1 }).rem_euclid(options.len() as i64) as usize].clone();
                }
            }
            Ctl::SetupSave if activate => core.lock().save_setup(),
            Ctl::SetupCancel if activate => core.lock().setup_form = None,
            _ => return false
        }
        true
    }
}

fn chooser_list(st: &State) -> Vec<crate::ble::Advertisement> {
    st.discovered.iter().filter(|a| a.is_lamp && st.dev(&a.id).is_none()).cloned().collect()
}

fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let t = text.trim().trim_start_matches('#');
    if t.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

fn shift_time(f: &mut AlarmForm, minutes: i64) {
    let (h, m) = crate::core::parse_time(&f.time).unwrap_or((0, 0));
    let total = (h as i64 * 60 + m as i64 + minutes).rem_euclid(24 * 60);
    f.time = format!("{:02}:{:02}", total / 60, total % 60);
}

fn draw_help(f: &mut Frame, area: Rect) {
    let text = vec![
        Line::from(Span::styled("Keys", Style::default().add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::raw("1-8, Tab, Shift+Tab   switch screens"),
        Line::raw("↑ ↓ (j k), PgUp PgDn  move between controls"),
        Line::raw("← → (h l)             change a value, slider ±1"),
        Line::raw("Shift+← →, [ ]        slider ±10 · Home/End min/max"),
        Line::raw("Enter / Space         activate, pick, type a value"),
        Line::raw("p                     power on/off for the controlled lamps"),
        Line::raw("a                     add a lamp (Devices screen)"),
        Line::raw("r / x                 rename / remove the focused lamp, favourite or setup"),
        Line::raw("mouse                 click the colour wheel to pick a colour"),
        Line::raw("q, Ctrl+C             quit"),
        Line::raw(""),
        Line::from(dim("Commands go to the lamps ticked on the Devices screen. Only lamps of the same type can be controlled together.")),
        Line::from(dim("Any key closes this help.")),
    ];
    let w = 84.min(area.width.saturating_sub(4));
    let h = (text.len() as u16 + 2).min(area.height);
    let r = Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height.saturating_sub(h)) / 2, width: w, height: h };
    f.render_widget(Clear, r);
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).block(Block::bordered().border_style(Style::default().fg(CLAY))), r);
}

// Hue/saturation wheel drawn with half blocks: two vertical pixels per cell, value fixed at 1 like the browser.
struct Wheel {
    hue: f64,
    sat: f64,
    rgb: [u8; 3]
}

impl Widget for Wheel {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = area.width as f64;
        let c = w / 2.0;
        let pixel = |x: f64, y: f64| -> Option<Color> {
            let dx = x + 0.5 - c;
            let dy = c - (y + 0.5);
            let dist = (dx * dx + dy * dy).sqrt() / c;
            if dist > 1.0 {
                return None;
            }
            let mut h = dy.atan2(dx).to_degrees();
            if h < 0.0 {
                h += 360.0;
            }
            Some(rgb(hsv_to_rgb(h, dist, 1.0)))
        };
        let a = self.hue.to_radians();
        let mx = (c + a.cos() * self.sat * c).clamp(0.0, w - 1.0) as u16;
        let my = ((c - a.sin() * self.sat * c) / 2.0).clamp(0.0, area.height as f64 - 1.0) as u16;
        for cy in 0..area.height {
            for cx in 0..area.width {
                let top = pixel(cx as f64, cy as f64 * 2.0);
                let bottom = pixel(cx as f64, cy as f64 * 2.0 + 1.0);
                let cell = &mut buf[(area.x + cx, area.y + cy)];
                match (top, bottom) {
                    (None, None) => continue,
                    (t, b) => {
                        cell.set_symbol("▀");
                        cell.set_fg(t.unwrap_or(Color::Reset));
                        cell.set_bg(b.unwrap_or(Color::Reset));
                    }
                }
                if cx == mx && cy == my {
                    let light = self.rgb.iter().map(|v| *v as u32).sum::<u32>() > 380;
                    cell.set_symbol("◉");
                    cell.set_fg(if light { Color::Black } else { Color::White });
                    cell.set_bg(rgb(self.rgb));
                }
            }
        }
    }
}
