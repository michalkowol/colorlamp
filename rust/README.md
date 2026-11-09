# raingel (terminal)

TUI and CLI for the same Raingel lamps as `../index.html`. It uses the same protocol and the same mechanisms (write queue, coalescing sender, static segments painted top first with a 3 s pause, length restore before lighting commands, phone microphone rhythm, reconnecting). See `../README.md` for the protocol.

## Building

```sh
cargo build --release
./target/release/raingel
```

Linux needs BlueZ, D-Bus and ALSA headers: `sudo apt install pkg-config libdbus-1-dev libasound2-dev`. On macOS the terminal app must be allowed to use Bluetooth (System Settings, Privacy and Security, Bluetooth) and the microphone for the rhythm.

## TUI

`raingel` without arguments starts the TUI. Screens: Devices, Colour, Modes, Rhythm, Schedule, Settings, Favourites, Setups.

| Key | Action |
|---|---|
| `1`-`8`, `Tab`, `Shift+Tab` | switch screens |
| `↑` `↓`, `PgUp` `PgDn` | move between controls |
| `←` `→` | change a value, slider ±1 |
| `Shift+←` `→`, `[` `]`, `Home` `End` | slider ±10, min, max |
| `Enter`, `Space` | activate, pick, type an exact value |
| `p` | power on/off |
| `a` | add a lamp (Devices) |
| `r`, `x` | rename, remove the focused lamp, favourite or setup |
| mouse | click the colour wheel |
| `?` | help |
| `q`, `Ctrl+C` | quit |

Commands go to the lamps ticked on the Devices screen. Only lamps of the same type can be controlled together.

## CLI

Commands connect to the lamps, send and disconnect. Without `--lamp` they go to all known lamps. `-v` prints every packet.

```sh
raingel scan                          # nearby lamps
raingel add AA:BB:CC:DD:EE:FF         # remember a lamp, read its type
raingel rename AA:BB:CC:DD:EE:FF Desk
raingel type Desk 16                  # set the type by hand when the advertisement has none
raingel on
raingel -l Desk brightness 60
raingel color amber                   # preset, #ffc000 or 255,192,0
raingel temp 0                        # 0 warm, 255 cold
raingel white 200
raingel modes                         # numbered list for the lamp type
raingel mode meteor 3 --speed 40 --brightness 80
raingel rhythm lamp --effect energy --sensitivity 70
raingel rhythm mic --color rotate     # computer microphone, Ctrl+C stops
raingel segments set blue red:42 --length 84   # top first: blue on top, red up to LED 42
raingel segments apply                # one segment every 3 s
raingel segments restore
raingel schedule add 07:30 on --days weekdays
raingel delay set 0 30
raingel order GRB
raingel length 120
raingel fav add-color red --brightness 80
raingel fav apply 1
raingel setup create Evening --set Desk=1 --set Shelf=off
raingel setup apply Evening
raingel raw 01 01
```

`raingel help <command>` shows all options.

## State

Settings, segments, favourites, setups, schedules, the delay and known lamps (name, type, shortened LED count) live in `~/.config/raingel/state.json` (Linux) or `~/Library/Application Support/raingel/state.json` (macOS). Override it with `--state` or `RAINGEL_STATE`. The TUI and the CLI share it.

## Differences from the browser

- Lamps are found by scanning without a filter and matching the advertised service `0xAF30` or the manufacturer data company id `22872`, which the browser chooser did with two filters. The manufacturer data is always visible here, so the type is read more reliably.
- Known lamps are kept in the state file instead of the browser permission store. They connect at start and reconnect when they advertise again, with the same backoff (3 s steps, up to 30 s, 10 attempts).
- The colour wheel is also available as hue and saturation sliders and a hex field.
- "This device's microphone" uses the default input device of the computer (cpal), with the same 2048 sample window and the same `20 * log10(mean |sample|) * 2.55` level.
- A setup sends to all its lamps at the same time, so lamps with segments do not wait for each other. The browser applies the lamps one after another.
