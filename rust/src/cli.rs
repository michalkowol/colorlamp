use crate::core::{AlarmForm, DelayForm, MicSource, Shared};
use crate::protocol::*;
use crate::store::FavoriteKind;
use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Parser)]
#[command(name = "raingel", version, about = "Control Raingel Bluetooth LE lamps and LED strips from a TUI or the command line")]
pub struct Cli {
    /// Lamp to control: id, name or Bluetooth name (repeat for several). Default: all known lamps
    #[arg(short, long, global = true)]
    pub lamp: Vec<String>,
    /// State file (default: ~/.config/raingel/state.json or $RAINGEL_STATE)
    #[arg(long, global = true)]
    pub state: Option<PathBuf>,
    /// Print every packet sent to the lamps
    #[arg(short, long, global = true)]
    pub verbose: bool,
    /// Seconds to wait for lamps to connect
    #[arg(long, global = true, default_value_t = 20)]
    pub timeout: u64,
    #[command(subcommand)]
    pub command: Option<Command>
}

#[derive(Clone, Copy, ValueEnum)]
pub enum OnOff {
    On,
    Off
}

impl OnOff {
    fn on(self) -> bool {
        matches!(self, OnOff::On)
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Interactive terminal UI (default)
    Tui,
    /// Scan for nearby lamps
    Scan {
        #[arg(long, default_value_t = 6)]
        seconds: u64
    },
    /// List remembered lamps
    Lamps,
    /// Remember a lamp found by `scan`, connect to it and read its type
    Add { id: String },
    /// Forget a lamp
    Forget { lamp: String },
    /// Rename a lamp (an empty name restores the Bluetooth name)
    Rename { lamp: String, name: String },
    /// Set the lamp type by hand: number, name or `unknown` (see `types`)
    Type { lamp: String, group: String },
    /// List lamp types
    Types,
    /// Turn on
    On,
    /// Turn off
    Off,
    /// Brightness 0-100
    Brightness { value: u8 },
    /// RGB colour: preset name, #rrggbb or r,g,b
    Color { value: String },
    /// List preset colours
    Colors,
    /// Colour temperature 0 (warm) - 255 (cold)
    Temp { value: u8 },
    /// White channel 0-255
    White { value: u8 },
    /// Sphere motor
    Motor {
        state: OnOff,
        #[arg(long, default_value_t = 50)]
        speed: u8
    },
    /// Sphere stars (laser)
    Laser {
        state: OnOff,
        #[arg(long, default_value_t = 50)]
        speed: u8
    },
    /// List modes (animations) for the lamp type
    Modes {
        /// Only this scene (name or number)
        scene: Option<String>
    },
    /// Start a mode: `mode <scene> <n>`, or `mode <n>` on two path lamps (numbers from `modes`)
    Mode {
        #[arg(num_args = 1..=2, required = true)]
        args: Vec<String>,
        #[arg(long, default_value_t = 0)]
        speed: u8,
        #[arg(long, default_value_t = 100)]
        brightness: u8
    },
    /// Music rhythm
    #[command(subcommand)]
    Rhythm(RhythmCommand),
    /// Fixed colour segments on Magic strips
    #[command(subcommand)]
    Segments(SegmentsCommand),
    /// Schedule: up to 6 active on/off entries stored in the lamp
    #[command(subcommand)]
    Schedule(ScheduleCommand),
    /// Countdown after which the lamp runs its timer action
    #[command(subcommand)]
    Delay(DelayCommand),
    /// Channel order: RGB, RBG, GRB, GBR, BRG or BGR
    Order { order: String },
    /// Number of controlled LEDs (IC length), 16-2048
    Length { value: i64 },
    /// Send the current time to the lamps
    TimeSync,
    /// Favourites
    #[command(subcommand)]
    Fav(FavCommand),
    /// Setups: one favourite (or off) per lamp, applied together
    #[command(subcommand)]
    Setup(SetupCommand),
    /// Send exactly these bytes, for example `raw 01 01` (no full length restore)
    Raw {
        #[arg(required = true)]
        bytes: Vec<String>
    }
}

#[derive(Subcommand)]
pub enum RhythmCommand {
    /// Lamp microphone
    Lamp {
        /// Effect name or number (see `rhythm effects`)
        #[arg(long, default_value = "1")]
        effect: String,
        #[arg(long, default_value_t = 50)]
        sensitivity: u8
    },
    /// This computer's microphone, sent every 150 ms until Ctrl+C
    Mic {
        #[arg(long, default_value = "1")]
        effect: String,
        /// `rotate` or a preset colour name
        #[arg(long, default_value = "rotate")]
        color: String,
        /// Stop after this many seconds
        #[arg(long)]
        seconds: Option<u64>
    },
    /// List effects for the lamp type
    Effects
}

#[derive(Subcommand)]
pub enum SegmentsCommand {
    /// Show the segments and the packets that will be sent
    Show,
    /// Set the segments, top first: `segments set blue red:42` (top blue, bottom red up to LED 42)
    Set {
        #[arg(required = true, num_args = 2..=6)]
        segments: Vec<String>,
        /// Full LED count of the strip
        #[arg(long)]
        length: Option<i64>
    },
    /// Set the full LED count of the strip and split the segments evenly
    Length { value: i64 },
    /// Split the strip into equal parts
    Split,
    /// Add a segment at the bottom (splits the bottom segment in half)
    Add,
    /// Remove a segment (number as in `show`, 1 is the top)
    Remove { number: usize },
    /// Send all segments, one every 3 s; --only sends a single segment
    Apply {
        #[arg(long)]
        only: Option<usize>
    },
    /// Restore the full LED count
    Restore
}

#[derive(Subcommand)]
pub enum ScheduleCommand {
    List,
    /// Add an entry: `schedule add 07:30 on --days weekdays`
    Add {
        time: String,
        action: OnOff,
        /// mon,tue,... or weekdays, weekend, daily, none
        #[arg(long, default_value = "none")]
        days: String,
        #[arg(long)]
        inactive: bool
    },
    /// Change an entry (number from `list`)
    Edit {
        number: usize,
        #[arg(long)]
        time: Option<String>,
        #[arg(long)]
        action: Option<OnOff>,
        #[arg(long)]
        days: Option<String>,
        #[arg(long)]
        active: Option<OnOff>
    },
    Remove { number: usize },
    Enable { number: usize },
    Disable { number: usize },
    /// Send the entries to the lamps again
    Sync
}

#[derive(Subcommand)]
pub enum DelayCommand {
    Show,
    /// Set and send: `delay set 0 30`
    Set {
        hours: i64,
        minutes: i64,
        #[arg(long)]
        inactive: bool
    },
    Enable,
    Disable,
    Remove
}

#[derive(Subcommand)]
pub enum FavCommand {
    List,
    /// Apply a favourite (number from `list` or name)
    Apply { favorite: String },
    Remove { favorite: String },
    Rename { favorite: String, name: String },
    /// Save an RGB colour (preset name, #rrggbb or r,g,b)
    AddColor {
        value: String,
        #[arg(long)]
        brightness: Option<u8>
    },
    /// Save a colour temperature 0-255
    AddTemp {
        value: u8,
        #[arg(long)]
        brightness: Option<u8>
    },
    /// Save a white channel value 0-255
    AddWhite {
        value: u8,
        #[arg(long)]
        brightness: Option<u8>
    },
    /// Save a mode: `fav add-mode <scene> <n>` (numbers from `modes`)
    AddMode {
        #[arg(num_args = 1..=2, required = true)]
        args: Vec<String>,
        #[arg(long, default_value_t = 0)]
        speed: u8,
        #[arg(long, default_value_t = 100)]
        brightness: u8
    },
    /// Save the current segments
    AddSegments
}

#[derive(Subcommand)]
pub enum SetupCommand {
    List,
    /// Apply a setup (number from `list` or name)
    Apply { setup: String },
    Remove { setup: String },
    Rename { setup: String, name: String },
    /// Create a setup: `setup create Evening --set desk=2 --set shelf=off`
    Create {
        name: String,
        /// lamp=favourite or lamp=off
        #[arg(long = "set", required = true)]
        lamps: Vec<String>
    }
}

pub async fn run(core: Shared, cli: Cli, command: Command) -> Result<()> {
    {
        let mut st = core.lock();
        st.echo_toast = true;
        st.echo_log = cli.verbose;
        st.confirm_writes = true;
        let known: Vec<(String, String)> = st.store.devices.iter().map(|d| (d.id.clone(), d.ble_name.clone())).collect();
        for (id, name) in known {
            st.register_device(&id, Some(name));
        }
    }
    let timeout = Duration::from_secs(cli.timeout);
    match command {
        Command::Tui => unreachable!(),
        Command::Scan { seconds } => scan(&core, seconds).await,
        Command::Lamps => {
            let st = core.lock();
            if st.devices.is_empty() {
                println!("No lamps. Run `raingel scan`, then `raingel add <id>`.");
            }
            for d in &st.devices {
                let group = d.group.map(group_name).unwrap_or_else(|| "type: unknown".into());
                let ble = if d.name != d.ble_name { format!(" ({})", d.ble_name) } else { String::new() };
                println!("{}  {}{ble}  {group}{}", d.id, d.name, if d.shortened { "  [LED count shortened]" } else { "" });
            }
            Ok(())
        }
        Command::Add { id } => {
            core.start().await;
            core.ble.set_scanning(true).await?;
            let deadline = Instant::now() + timeout;
            let name = loop {
                if let Some(a) = core.ble.advertisement(&id).await {
                    break a.name;
                }
                if Instant::now() > deadline {
                    bail!("Lamp {id} not found. Run `raingel scan` first.");
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            };
            core.add_device(&id, name).await;
            tokio::time::sleep(Duration::from_secs(2)).await;
            let line = {
                let st = core.lock();
                st.dev(&id).map(|d| format!("{}  {}  {}  {}", d.id, d.name, d.group.map(group_name).unwrap_or_default(), d.status()))
            };
            println!("{}", line.ok_or_else(|| anyhow!("Lamp {id} not registered"))?);
            core.disconnect_and_wait(&id).await;
            Ok(())
        }
        Command::Forget { lamp } => {
            let id = resolve_lamp(&core, &lamp)?;
            let mut st = core.lock();
            st.devices.retain(|d| d.id != id);
            st.store.devices.retain(|d| d.id != id);
            st.persist();
            Ok(())
        }
        Command::Rename { lamp, name } => {
            let id = resolve_lamp(&core, &lamp)?;
            core.lock().rename_device(&id, &name);
            Ok(())
        }
        Command::Type { lamp, group } => {
            let id = resolve_lamp(&core, &lamp)?;
            let g = parse_group(&group)?;
            core.lock().set_group(&id, g);
            Ok(())
        }
        Command::Types => {
            for (id, name) in GROUPS {
                println!("{id:>3}  {name}");
            }
            Ok(())
        }
        Command::Colors => {
            for i in PALETTE_ORDER {
                let c = CLASSIC_RGB[i];
                println!("\x1b[38;2;{};{};{}m██\x1b[0m  {:<13} #{:02x}{:02x}{:02x}", c[0], c[1], c[2], CLASSIC_NAMES[i], c[0], c[1], c[2]);
            }
            Ok(())
        }
        Command::Modes { scene } => {
            select_targets(&core, &cli.lamp)?;
            print_modes(&core, scene.as_deref())
        }
        Command::Rhythm(RhythmCommand::Effects) => {
            select_targets(&core, &cli.lamp)?;
            let st = core.lock();
            for (i, e) in st.mic_effects().iter().enumerate() {
                println!("{}  {e}", i + 1);
            }
            Ok(())
        }
        Command::Segments(cmd) if !matches!(cmd, SegmentsCommand::Apply { .. } | SegmentsCommand::Restore) => {
            select_targets(&core, &cli.lamp)?;
            segments_offline(&core, cmd)
        }
        Command::Schedule(ScheduleCommand::List) => {
            let st = core.lock();
            if st.store.alarms.is_empty() {
                println!("No schedules. The lamp stores up to 6 active entries.");
            }
            for (i, a) in st.store.alarms.iter().enumerate() {
                println!(
                    "{}  {:02}:{:02}  {:<8}  {:<12} {}",
                    i + 1,
                    a.hour,
                    a.minute,
                    if a.on { "turn on" } else { "turn off" },
                    weeks_text(a.weeks),
                    if a.enable { "active" } else { "inactive" }
                );
            }
            Ok(())
        }
        Command::Delay(DelayCommand::Show) => {
            match &core.lock().store.delay {
                Some(d) => println!("{:02}:{:02}  {}", d.hour, d.minute, if d.enable { "countdown active" } else { "off" }),
                None => println!("No delay.")
            }
            Ok(())
        }
        Command::Fav(cmd) if !matches!(cmd, FavCommand::Apply { .. }) => {
            select_targets(&core, &cli.lamp)?;
            favorites_offline(&core, cmd)
        }
        Command::Setup(SetupCommand::Apply { setup }) => {
            let s = {
                let st = core.lock();
                let i = find_index(&st.store.setups.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), &setup)?;
                st.store.setups[i].clone()
            };
            let ids: Vec<String> = s.lamps.iter().map(|l| l.device.clone()).collect();
            online(&core, &ids, timeout).await?;
            core.apply_setup(&s).await;
            finish(&core, &ids).await;
            Ok(())
        }
        Command::Setup(cmd) => setups_offline(&core, cmd),
        command => {
            let ids = select_targets(&core, &cli.lamp)?;
            prepare(&core, &command)?;
            online(&core, &ids, timeout).await?;
            run_online(&core, command).await?;
            finish(&core, &ids).await;
            Ok(())
        }
    }
}

async fn scan(core: &Shared, seconds: u64) -> Result<()> {
    core.start().await;
    if !core.ble.available() {
        bail!(core.lock().ble_error.clone().unwrap_or_else(|| "No Bluetooth adapter found.".into()));
    }
    core.set_choosing(true);
    println!("Scanning for {seconds} s…");
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    core.set_choosing(false);
    let st = core.lock();
    let lamps: Vec<_> = st.discovered.iter().filter(|a| a.is_lamp).collect();
    if lamps.is_empty() {
        println!("No lamps found. Turn the lamp on and keep it close.");
    }
    for a in lamps {
        let group = a.manufacturer.as_ref().filter(|m| m.len() >= 2).map(|m| format!("{} (type {})", group_name(m[0]), m[1])).unwrap_or_else(|| "type unknown".into());
        let known = if st.store.device(&a.id).is_some() { "  [known]" } else { "" };
        println!(
            "{}  {:<20} {:>7}  {group}{known}",
            a.id,
            a.name.clone().unwrap_or_else(|| "Lamp".into()),
            a.rssi.map(|r| format!("{r} dBm")).unwrap_or_default()
        );
    }
    Ok(())
}

fn resolve_lamp(core: &Shared, query: &str) -> Result<String> {
    let st = core.lock();
    let q = query.to_lowercase();
    st.devices
        .iter()
        .find(|d| d.id.to_lowercase() == q)
        .or_else(|| st.devices.iter().find(|d| d.name.to_lowercase() == q))
        .or_else(|| st.devices.iter().find(|d| d.ble_name.to_lowercase() == q))
        .map(|d| d.id.clone())
        .ok_or_else(|| anyhow!("Unknown lamp {query}. See `raingel lamps`."))
}

// Selects the lamps to control, with the same rule as the browser: one lamp type at a time.
fn select_targets(core: &Shared, lamps: &[String]) -> Result<Vec<String>> {
    let ids: Vec<String> = if lamps.is_empty() {
        core.lock().devices.iter().map(|d| d.id.clone()).collect()
    } else {
        lamps.iter().map(|l| resolve_lamp(core, l)).collect::<Result<_>>()?
    };
    let mut st = core.lock();
    for d in st.devices.iter_mut() {
        d.selected = false;
    }
    for id in &ids {
        st.toggle_select(id);
        if !st.dev(id).map(|d| d.selected).unwrap_or(false) {
            bail!("Only lamps of the same type can be controlled together. Pick them with --lamp.");
        }
    }
    st.sync_group();
    Ok(ids)
}

async fn online(core: &Shared, ids: &[String], timeout: Duration) -> Result<()> {
    if ids.is_empty() {
        bail!("No lamps. Run `raingel scan`, then `raingel add <id>`.");
    }
    core.start().await;
    if !core.ble.available() {
        bail!(core.lock().ble_error.clone().unwrap_or_else(|| "No Bluetooth adapter found.".into()));
    }
    core.ble.set_scanning(true).await?;
    let deadline = Instant::now() + timeout;
    let tasks = ids.iter().map(|id| {
        let core = core.clone();
        let id = id.clone();
        async move {
            while Instant::now() < deadline {
                if core.connect(&id, true).await {
                    return true;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            false
        }
    });
    let results = futures::future::join_all(tasks).await;
    let _ = core.ble.set_scanning(false).await;
    let st = core.lock();
    let mut failed = Vec::new();
    for (id, ok) in ids.iter().zip(&results) {
        if !ok {
            failed.push(st.dev(id).map(|d| d.name.clone()).unwrap_or_else(|| id.clone()));
        }
    }
    if failed.len() == ids.len() {
        bail!("Could not connect to {}.", failed.join(", "));
    }
    if !failed.is_empty() {
        eprintln!("Could not connect to {}, skipped.", failed.join(", "));
    }
    if st.echo_log {
        let names: Vec<String> = ids.iter().zip(&results).filter(|(_, ok)| **ok).map(|(id, _)| st.dev(id).map(|d| d.name.clone()).unwrap_or_else(|| id.clone())).collect();
        eprintln!("Connected: {}", names.join(", "));
    }
    Ok(())
}

// Writes without response only reach the local Bluetooth stack when they return. Disconnecting
// right away can drop them, so give the stack time to send them first.
const SETTLE: Duration = Duration::from_millis(500);

async fn finish(core: &Shared, ids: &[String]) {
    core.idle().await;
    for id in ids {
        core.ble.barrier(id).await;
    }
    tokio::time::sleep(SETTLE).await;
    futures::future::join_all(ids.iter().map(|id| core.disconnect_and_wait(id))).await;
}

fn parse_group(text: &str) -> Result<Option<u8>> {
    if text.eq_ignore_ascii_case("unknown") {
        return Ok(None);
    }
    if let Ok(n) = text.parse::<u8>() {
        return Ok(Some(n));
    }
    let q = text.to_lowercase();
    GROUPS
        .iter()
        .find(|(_, n)| n.to_lowercase() == q)
        .or_else(|| GROUPS.iter().find(|(_, n)| n.to_lowercase().starts_with(&q)))
        .map(|(id, _)| Some(*id))
        .ok_or_else(|| anyhow!("Unknown lamp type {text}. See `raingel types`."))
}

pub fn parse_color(text: &str) -> Result<[u8; 3]> {
    if let Some(i) = palette_index_by_name(text) {
        return Ok(CLASSIC_RGB[i]);
    }
    let t = text.trim().trim_start_matches('#');
    if t.len() == 6 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        let b = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).unwrap();
        return Ok([b(0), b(2), b(4)]);
    }
    let parts: Vec<&str> = text.split(',').map(|p| p.trim()).collect();
    if parts.len() == 3 {
        let v: Vec<u8> = parts.iter().map(|p| p.parse::<u8>()).collect::<Result<_, _>>().map_err(|_| anyhow!("Invalid colour {text}"))?;
        return Ok([v[0], v[1], v[2]]);
    }
    bail!("Invalid colour {text}. Use a preset name (see `raingel colors`), #rrggbb or r,g,b.")
}

fn parse_days(text: &str) -> Result<u8> {
    match text.to_lowercase().as_str() {
        "none" | "" => return Ok(0),
        "daily" | "every-day" | "everyday" | "all" => return Ok(127),
        "weekdays" => return Ok(31),
        "weekend" => return Ok(96),
        _ => {}
    }
    let mut w = 0;
    for part in text.split(',') {
        let p = part.trim().to_lowercase();
        let (_, bit) = WEEKDAYS
            .iter()
            .find(|(n, _)| n.to_lowercase() == p || n.to_lowercase() == p.chars().take(3).collect::<String>())
            .ok_or_else(|| anyhow!("Unknown day {part}. Use mon,tue,wed,thu,fri,sat,sun or weekdays, weekend, daily, none."))?;
        w |= bit;
    }
    Ok(w)
}

fn find_index(names: &[String], query: &str) -> Result<usize> {
    if let Ok(n) = query.parse::<usize>()
        && n >= 1
        && n <= names.len()
    {
        return Ok(n - 1);
    }
    let q = query.to_lowercase();
    names.iter().position(|n| n.to_lowercase() == q).ok_or_else(|| anyhow!("Not found: {query}"))
}

fn parse_effect(core: &Shared, text: &str) -> Result<u8> {
    let st = core.lock();
    let names: Vec<String> = st.mic_effects().iter().map(|s| s.to_string()).collect();
    Ok(find_index(&names, text).map_err(|_| anyhow!("Unknown effect {text}. See `raingel rhythm effects`."))? as u8)
}

// Resolves `<scene> <n>` (or `<n>` on two path lamps) to the scene tab and mode index.
fn parse_mode(core: &Shared, args: &[String]) -> Result<(usize, usize)> {
    let st = core.lock();
    let scenes = st.scene_tabs();
    let (scene, item) = if scenes.is_empty() {
        if args.len() != 1 {
            bail!("This lamp type has no scenes: use `mode <n>`.");
        }
        (0, &args[0])
    } else {
        if args.len() != 2 {
            bail!("Use `mode <scene> <n>`, see `raingel modes`.");
        }
        let names: Vec<String> = scenes.iter().map(|s| s.to_string()).collect();
        (find_index(&names, &args[0]).map_err(|_| anyhow!("Unknown scene {}. See `raingel modes`.", args[0]))?, &args[1])
    };
    let count = mode_items(st.group(), scene).len();
    let n: usize = item.parse().map_err(|_| anyhow!("Invalid mode number {item}"))?;
    if n < 1 || n > count {
        bail!("Mode number must be 1-{count}.");
    }
    Ok((scene, n - 1))
}

fn print_modes(core: &Shared, scene: Option<&str>) -> Result<()> {
    let st = core.lock();
    let group = st.group();
    println!("Lamp type: {}", group_name(group));
    let scenes = st.scene_tabs();
    let names: Vec<String> = scenes.iter().map(|s| s.to_string()).collect();
    let list: Vec<usize> = match scene {
        Some(s) if !names.is_empty() => vec![find_index(&names, s)?],
        _ if names.is_empty() => vec![0],
        _ => (0..names.len()).collect()
    };
    for s in list {
        if !names.is_empty() {
            println!("\n{}. {}", s + 1, names[s]);
        }
        for (i, item) in mode_items(group, s).iter().enumerate() {
            let dots: String = item.dots.iter().map(|d| format!("\x1b[38;2;{};{};{}m●\x1b[0m", d.rgb[0], d.rgb[1], d.rgb[2])).collect();
            let colours: Vec<&str> = item.dots.iter().map(|d| d.name).collect();
            println!("  {:>2}  {dots} {} {}", i + 1, colours.join(" "), item.label);
        }
    }
    Ok(())
}

fn segments_offline(core: &Shared, cmd: SegmentsCommand) -> Result<()> {
    {
        let mut st = core.lock();
        match cmd {
            SegmentsCommand::Show => {}
            SegmentsCommand::Set { segments, length } => {
                if let Some(l) = length {
                    st.store.settings.ic_length = l;
                }
                let mut list = Vec::new();
                let n = segments.len();
                for (k, spec) in segments.iter().enumerate() {
                    let (color, end) = match spec.rsplit_once(':') {
                        Some((c, e)) => (c, Some(e.parse::<i64>().map_err(|_| anyhow!("Invalid boundary in {spec}"))?)),
                        None => (spec.as_str(), None)
                    };
                    let index = palette_index_by_name(color).ok_or_else(|| anyhow!("Unknown colour {color}. See `raingel colors`."))?;
                    if k > 0 && end.is_none() {
                        bail!("Segment {}: give the boundary as colour:LED, for example red:42.", k + 1);
                    }
                    list.push(crate::store::Segment { color: index, end: if k == 0 { None } else { end } });
                    let _ = n;
                }
                list.reverse();
                st.store.segments = list;
                st.persist();
            }
            SegmentsCommand::Length { value } => {
                st.store.settings.ic_length = value;
                st.split_segments_evenly();
            }
            SegmentsCommand::Split => st.split_segments_evenly(),
            SegmentsCommand::Add => {
                if st.store.segments.len() >= 6 {
                    bail!("At most 6 segments.");
                }
                st.add_segment();
            }
            SegmentsCommand::Remove { number } => {
                let n = st.store.segments.len();
                if number < 1 || number > n {
                    bail!("Segment number must be 1-{n}.");
                }
                if n <= 2 {
                    bail!("At least 2 segments.");
                }
                st.remove_segment(n - number);
            }
            SegmentsCommand::Apply { .. } | SegmentsCommand::Restore => unreachable!()
        }
    }
    let st = core.lock();
    let n = st.store.segments.len();
    println!("Full LED count: {}", st.store.settings.ic_length);
    for i in (0..n).rev() {
        let seg = &st.store.segments[i];
        let end = if i == n - 1 { st.store.settings.ic_length.to_string() } else { seg.end.map(|e| e.to_string()).unwrap_or_else(|| "?".into()) };
        let c = CLASSIC_RGB[seg.color.min(15)];
        let label = if i == n - 1 { " (top)" } else if i == 0 { " (bottom)" } else { "" };
        println!(
            "  {}{label:<9} \x1b[38;2;{};{};{}m██\x1b[0m {:<13} up to LED {end:<5} {}",
            n - i,
            c[0],
            c[1],
            c[2],
            CLASSIC_NAMES[seg.color.min(15)],
            st.segment_percent(i)
        );
    }
    match st.segments_error() {
        Some(e) => println!("{e}"),
        None => {
            println!("Steps to be sent:");
            for (k, step) in st.segment_steps().iter().enumerate() {
                println!("  {}. {}  {}", k + 1, step.label, step.packets.iter().map(|p| to_hex(p)).collect::<Vec<_>>().join("  ·  "));
            }
        }
    }
    if st.segments_active() {
        println!("The number of controlled LEDs is currently shortened.");
    }
    Ok(())
}

fn apply_brightness(core: &Shared, brightness: Option<u8>) {
    if let Some(b) = brightness {
        let mut st = core.lock();
        st.store.settings.brightness = b.min(100);
        st.persist();
    }
}

fn favorites_offline(core: &Shared, cmd: FavCommand) -> Result<()> {
    let mut st = core.lock();
    let names: Vec<String> = st.store.favorites.iter().map(|f| f.name.clone()).collect();
    match cmd {
        FavCommand::List => {
            if st.store.favorites.is_empty() {
                println!("No favourites.");
            }
            for (i, f) in st.store.favorites.iter().enumerate() {
                let kind = match f.kind {
                    FavoriteKind::Color { .. } => "colour",
                    FavoriteKind::Mode { .. } => "mode",
                    FavoriteKind::Segments { .. } => "segments"
                };
                println!("{}  {:<32} {:<9} {}", i + 1, f.name, kind, st.favorite_details(f));
            }
        }
        FavCommand::Remove { favorite } => {
            let id = st.store.favorites[find_index(&names, &favorite)?].id.clone();
            st.remove_favorite(&id);
        }
        FavCommand::Rename { favorite, name } => {
            let id = st.store.favorites[find_index(&names, &favorite)?].id.clone();
            st.rename_favorite(&id, &name);
        }
        FavCommand::AddColor { value, brightness } => {
            let c = parse_color(&value)?;
            drop(st);
            apply_brightness(core, brightness);
            let mut st = core.lock();
            st.color_tab = ColorTab::Color;
            st.rgb = c;
            let (h, s) = rgb_to_hs(c);
            st.hue = h;
            st.sat = s;
            if !st.can_save_color() {
                bail!("This lamp type has no RGB colour.");
            }
            st.add_color_favorite();
        }
        FavCommand::AddTemp { value, brightness } => {
            drop(st);
            apply_brightness(core, brightness);
            let mut st = core.lock();
            st.color_tab = ColorTab::Cct;
            st.cct = value;
            if !st.can_save_color() {
                bail!("This lamp type has no colour temperature.");
            }
            st.add_color_favorite();
        }
        FavCommand::AddWhite { value, brightness } => {
            drop(st);
            apply_brightness(core, brightness);
            let mut st = core.lock();
            st.color_tab = ColorTab::Wb;
            st.wb = value;
            if !st.can_save_color() {
                bail!("This lamp type has no white channel.");
            }
            st.add_color_favorite();
        }
        FavCommand::AddMode { args, speed, brightness } => {
            drop(st);
            let (scene, index) = parse_mode(core, &args)?;
            let mut st = core.lock();
            st.scene_tab = scene;
            st.mode_index = Some(index);
            st.speed = speed.min(100);
            st.mode_brightness = brightness.min(100);
            st.add_mode_favorite();
        }
        FavCommand::AddSegments => {
            if let Some(e) = st.segments_error() {
                bail!(e);
            }
            st.add_segments_favorite();
        }
        FavCommand::Apply { .. } => unreachable!()
    }
    Ok(())
}

fn setups_offline(core: &Shared, cmd: SetupCommand) -> Result<()> {
    let mut st = core.lock();
    let names: Vec<String> = st.store.setups.iter().map(|s| s.name.clone()).collect();
    match cmd {
        SetupCommand::List => {
            if st.store.setups.is_empty() {
                println!("No setups.");
            }
            for (i, s) in st.store.setups.iter().enumerate() {
                println!("{}  {:<24} {}", i + 1, s.name, st.setup_details(s));
            }
        }
        SetupCommand::Remove { setup } => {
            let id = st.store.setups[find_index(&names, &setup)?].id.clone();
            st.remove_setup(&id);
        }
        SetupCommand::Rename { setup, name } => {
            let id = st.store.setups[find_index(&names, &setup)?].id.clone();
            st.rename_setup(&id, &name);
        }
        SetupCommand::Create { name, lamps } => {
            let fav_names: Vec<String> = st.store.favorites.iter().map(|f| f.name.clone()).collect();
            let mut form_lamps = Vec::new();
            for spec in lamps {
                let (lamp, action) = spec.split_once('=').ok_or_else(|| anyhow!("Use lamp=favourite or lamp=off, got {spec}"))?;
                drop(st);
                let id = resolve_lamp(core, lamp)?;
                st = core.lock();
                let action = if action.eq_ignore_ascii_case("off") {
                    "off".to_string()
                } else {
                    st.store.favorites[find_index(&fav_names, action)?].id.clone()
                };
                form_lamps.push((id, action));
            }
            st.setup_form = Some(crate::core::SetupForm { name, lamps: form_lamps });
            st.save_setup();
        }
        SetupCommand::Apply { .. } => unreachable!()
    }
    Ok(())
}

// Validates the arguments before connecting, so a typo does not wait for the lamps.
fn prepare(core: &Shared, command: &Command) -> Result<()> {
    match command {
        Command::Color { value } => {
            parse_color(value)?;
        }
        Command::Mode { args, .. } => {
            parse_mode(core, args)?;
        }
        Command::Rhythm(RhythmCommand::Lamp { effect, .. }) => {
            parse_effect(core, effect)?;
        }
        Command::Rhythm(RhythmCommand::Mic { effect, color, .. }) => {
            parse_effect(core, effect)?;
            if !color.eq_ignore_ascii_case("rotate") && palette_index_by_name(color).is_none() {
                bail!("Unknown colour {color}. Use rotate or a preset name (see `raingel colors`).");
            }
        }
        Command::Segments(SegmentsCommand::Apply { only }) => {
            let st = core.lock();
            if let Some(e) = st.segments_error() {
                bail!(e);
            }
            if let Some(n) = only
                && (*n < 1 || *n > st.store.segments.len())
            {
                bail!("Segment number must be 1-{}.", st.store.segments.len());
            }
        }
        Command::Segments(SegmentsCommand::Restore) | Command::Length { .. } => {
            let full = match command {
                Command::Length { value } => *value,
                _ => core.lock().store.settings.ic_length
            };
            if !(MIN_LEDS..=MAX_LEDS).contains(&full) {
                bail!("The LED count must be between 16 and 2048.");
            }
        }
        Command::Order { order } => {
            if !IC_ORDERS.iter().any(|o| o.eq_ignore_ascii_case(order)) {
                bail!("Unknown order {order}. Use one of {}.", IC_ORDERS.join(", "));
            }
        }
        Command::Fav(FavCommand::Apply { favorite }) => {
            let st = core.lock();
            find_index(&st.store.favorites.iter().map(|f| f.name.clone()).collect::<Vec<_>>(), favorite)?;
        }
        Command::Schedule(ScheduleCommand::Add { time, days, .. }) => {
            if crate::core::parse_time(time).is_none() {
                bail!("Enter the time as HH:MM.");
            }
            parse_days(days)?;
            if core.lock().store.alarms.len() >= 6 {
                bail!("The lamp stores up to 6 entries.");
            }
        }
        Command::Schedule(ScheduleCommand::Edit { number, time, days, .. }) => {
            check_alarm(core, *number)?;
            if let Some(t) = time
                && crate::core::parse_time(t).is_none()
            {
                bail!("Enter the time as HH:MM.");
            }
            if let Some(d) = days {
                parse_days(d)?;
            }
        }
        Command::Schedule(ScheduleCommand::Remove { number } | ScheduleCommand::Enable { number } | ScheduleCommand::Disable { number }) => {
            check_alarm(core, *number)?;
        }
        Command::Delay(DelayCommand::Set { hours, minutes, .. }) => {
            if hours.clamp(&0, &23) == &0 && minutes.clamp(&0, &59) == &0 {
                bail!("Set at least 1 minute.");
            }
        }
        Command::Delay(DelayCommand::Enable | DelayCommand::Disable) => {
            if core.lock().store.delay.is_none() {
                bail!("No delay. Use `raingel delay set <hours> <minutes>`.");
            }
        }
        Command::Raw { bytes } => {
            parse_raw(bytes)?;
        }
        _ => {}
    }
    Ok(())
}

fn check_alarm(core: &Shared, number: usize) -> Result<()> {
    let n = core.lock().store.alarms.len();
    if number < 1 || number > n {
        bail!("Entry number must be 1-{n}.");
    }
    Ok(())
}

fn parse_raw(bytes: &[String]) -> Result<Vec<u8>> {
    let joined: String = bytes.join("").replace([' ', ':', ','], "");
    if joined.is_empty() || !joined.len().is_multiple_of(2) || !joined.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("Give the packet as hex bytes, for example `raw 01 01`.");
    }
    Ok(hex_bytes(&joined))
}

async fn run_online(core: &Shared, command: Command) -> Result<()> {
    match command {
        Command::On => core.set_power(true),
        Command::Off => core.set_power(false),
        Command::Brightness { value } => core.set_brightness(value),
        Command::Color { value } => core.set_rgb(parse_color(&value)?),
        Command::Temp { value } => core.set_cct(value),
        Command::White { value } => core.set_wb(value),
        Command::Motor { state, speed } => {
            {
                let mut st = core.lock();
                st.motor_on = state.on();
                st.motor_speed = speed.min(100);
            }
            core.set_motor();
        }
        Command::Laser { state, speed } => {
            {
                let mut st = core.lock();
                st.laser_on = state.on();
                st.laser_speed = speed.min(100);
            }
            core.set_laser();
        }
        Command::Mode { args, speed, brightness } => {
            let (scene, index) = parse_mode(core, &args)?;
            {
                let mut st = core.lock();
                st.speed = speed.min(100);
                st.mode_brightness = brightness.min(100);
            }
            core.pick_scene(scene);
            core.pick_mode(index);
        }
        Command::Rhythm(RhythmCommand::Lamp { effect, sensitivity }) => {
            let effect = parse_effect(core, &effect)?;
            {
                let mut st = core.lock();
                st.mic_source = MicSource::Device;
                st.sensitivity = sensitivity.min(100);
                st.mic_effect = effect;
            }
            core.send_device_mic();
        }
        Command::Rhythm(RhythmCommand::Mic { effect, color, seconds }) => {
            let effect = parse_effect(core, &effect)?;
            let mic_color = palette_index_by_name(&color).map(|i| i as i32).unwrap_or(-1);
            {
                let mut st = core.lock();
                st.mic_source = MicSource::Phone;
                st.mic_effect = effect;
            }
            core.set_mic_color(mic_color);
            core.start_phone_mic();
            if !core.lock().recording {
                bail!("No access to the microphone.");
            }
            println!("Listening, Ctrl+C to stop.");
            let start = Instant::now();
            loop {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => break,
                    _ = tokio::time::sleep(Duration::from_millis(150)) => {}
                }
                if let Some(s) = seconds
                    && start.elapsed() >= Duration::from_secs(s)
                {
                    break;
                }
                let level = core.lock().level as usize;
                eprint!("\r{:<50} {level:>3}%", "█".repeat(level / 2));
            }
            eprintln!();
            core.stop_phone_mic();
        }
        Command::Segments(SegmentsCommand::Apply { only }) => match only {
            Some(n) => {
                let total = core.lock().store.segments.len();
                core.apply_segment(total - n).await;
            }
            None => {
                println!("Sending {} segments, one every {} s…", core.lock().store.segments.len(), SEGMENT_PAUSE_MS / 1000);
                core.apply_segments().await;
            }
        },
        Command::Segments(SegmentsCommand::Restore) => core.restore_length(),
        Command::Schedule(cmd) => {
            match cmd {
                ScheduleCommand::Add { time, action, days, inactive } => {
                    core.lock().alarm_form = Some(AlarmForm { index: None, time, weeks: parse_days(&days)?, on: action.on(), enable: !inactive });
                    core.save_alarm();
                }
                ScheduleCommand::Edit { number, time, action, days, active } => {
                    core.lock().edit_alarm(number - 1);
                    {
                        let mut st = core.lock();
                        let f = st.alarm_form.as_mut().unwrap();
                        if let Some(t) = time {
                            f.time = t;
                        }
                        if let Some(a) = action {
                            f.on = a.on();
                        }
                        if let Some(d) = days {
                            f.weeks = parse_days(&d)?;
                        }
                        if let Some(a) = active {
                            f.enable = a.on();
                        }
                    }
                    core.save_alarm();
                }
                ScheduleCommand::Remove { number } => core.remove_alarm(number - 1),
                ScheduleCommand::Enable { number } | ScheduleCommand::Disable { number } => {
                    let want = matches!(cmd, ScheduleCommand::Enable { .. });
                    if core.lock().store.alarms[number - 1].enable != want {
                        core.toggle_alarm(number - 1);
                    } else {
                        core.sync_alarms();
                    }
                }
                ScheduleCommand::Sync => core.sync_alarms(),
                ScheduleCommand::List => unreachable!()
            }
        }
        Command::Delay(cmd) => match cmd {
            DelayCommand::Set { hours, minutes, inactive } => {
                core.lock().delay_form = Some(DelayForm { hour: hours, minute: minutes, enable: !inactive });
                core.save_delay();
            }
            DelayCommand::Enable | DelayCommand::Disable => {
                let want = matches!(cmd, DelayCommand::Enable);
                let current = core.lock().store.delay.as_ref().map(|d| d.enable);
                if current != Some(want) {
                    core.toggle_delay();
                } else {
                    core.sync_delay();
                }
            }
            DelayCommand::Remove => core.remove_delay(),
            DelayCommand::Show => unreachable!()
        },
        Command::Order { order } => {
            let i = IC_ORDERS.iter().position(|o| o.eq_ignore_ascii_case(&order)).unwrap();
            core.lock().store.settings.ic_order = i;
            core.send_ic_order();
        }
        Command::Length { value } => {
            core.lock().store.settings.ic_length = value;
            core.send_ic_length();
        }
        Command::TimeSync => {
            let ids = core.lock().target_devices();
            for id in ids {
                core.sync_time(&id).await;
            }
        }
        Command::Fav(FavCommand::Apply { favorite }) => {
            let f = {
                let st = core.lock();
                let i = find_index(&st.store.favorites.iter().map(|f| f.name.clone()).collect::<Vec<_>>(), &favorite)?;
                st.store.favorites[i].clone()
            };
            core.apply_favorite(&f).await;
        }
        // Exactly the given bytes: no full length restore in front of them.
        Command::Raw { bytes } => core.send(parse_raw(&bytes)?, true).await,
        _ => unreachable!()
    }
    Ok(())
}
