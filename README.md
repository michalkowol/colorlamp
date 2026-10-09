# Raingel lamp controller

A single page app (`index.html`, Alpine.js, Web Bluetooth) that controls LED lamps and strips made for the Raingel Android app. The protocol was taken from the decompiled APK `Raingel_release_C55_V202601071930.apk` (package `am.doit.dohome.strip`).

Live version: [michalkowol.github.io/colorlamp](https://michalkowol.github.io/colorlamp/)

## Running

Web Bluetooth needs Chrome, Edge or Opera (desktop or Android) and a secure context. Open the page from `https://` or `localhost`, for example `python3 -m http.server`. It does not work from `file://` or on iOS.

## Bluetooth

| Item | Value |
|---|---|
| Advertised service (scan filter) | `0xAF30` |
| GATT service | `0xAE30` |
| Write characteristic | `0xAE01`, write without response |
| Notify characteristic | `0xAE02`, never sends anything |
| Manufacturer data | company id `22872` (`0x5958`), 2 bytes: `[group, type]` |

The lamp never reports its state. The app only sends commands.

### Device groups

The group byte decides which features the lamp has.

| Group | Lamp |
|---|---|
| 1 | dimmer |
| 2 | CCT |
| 3 | RGB |
| 4 | RGBW |
| 5 | RGB + CCT |
| 6 | RGBCW |
| 16 | Magic (addressable) |
| 17, 18 | Magic, 2 paths |
| 19 | sphere (motor + laser) |
| 20 | Magic W |
| 21 | Magic CW |

Browsers often hide advertisement data, so the app falls back to group 16. The user can change it per lamp.

## Commands

All values are single unsigned bytes. 16-bit values are little endian (`lo hi`).

| Opcode | Packet | Meaning |
|---|---|---|
| `01` | `01 on` | power, `on` is 0 or 1 |
| `02` | `02 b` | brightness 0-100 |
| `03` | `03 r g b w c flag` | colour, see below |
| `04` | `04 effect sensitivity` | lamp microphone, effect 0-3, sensitivity 0-100 |
| `05` | `05 enabled 01 lo hi` | delay in minutes |
| `07` | `07 ...` | mode (animation), see below |
| `08` | `08 01 00 00 00 yl yh month day hour min sec` | clock sync, sent after connecting |
| `0D` | `0D ...` | alarms, see below |
| `0E` | `0E n` | channel order 1-6: RGB, RBG, GRB, GBR, BRG, BGR |
| `0F` | `0F lo hi` | number of controlled LEDs (IC length), 16-2048 |
| `10` | `10 r g b 00 00 level effect` | phone microphone frame, level 0-255 |
| `11` | `11 04` | start phone microphone mode |
| `15` | `15 on 00 speed` | sphere motor |
| `17` | `17 on 00 speed 64` | sphere laser |

### Colour (`03`)

- RGB: `03 r g b 00 00 00`
- Colour temperature: `03 00 00 00 00 c 01`, `c` 0 is warm and 255 is cold
- White channel: `03 00 00 00 w 00 01`

`03` stops any running mode. On Magic strips only fully saturated colours look right.

### Modes (`07`)

RGB and sphere lamps: `07 scene speed brightness count colours`

- `scene` is 1-4: fade, jump, breathe, flash
- `count` is the number of colours
- `colours` are colour indexes packed two per byte

Magic lamps: `07 mode speed brightness count colours`

- `count colours` is always 5 bytes, for example `02 01 00 00 00`
- `mode` is `id | section << 7 | direction << 6`
- `id` 1-18: fade, jump, breathe, flash, meteor, stack, float, follow spot, wave, water, rainbow, blink, bounce, shuttle, twinkle, on/off, curtain, alternate
- bit 6 means reverse direction, or "multicolour" for id 14, "alternate" for id 16, "curtain down" for id 17
- bit 7 means "by section" (ids 1-4)

Two path lamps: `07 mode speed brightness`, where `mode` is `(200 + i) & 31` for scene `i` 0-16.

Colour indexes: 0 red, 1 green, 2 blue, 3 yellow, 4 cyan, 5 violet, 6 orange, 7 white.

Speed and brightness are 0-100. A mode keeps running at speed 0. There is no way to freeze it.

### Alarms (`0D`)

Up to 6 enabled alarms, 3 bytes each: `hour | index << 5`, `minute | 0x80 if turn on`, `weekdays`. Weekday bits: Monday 1, Tuesday 2, ..., Sunday 64. With no alarms the packet is `0D 00 00`.

### Delay (`05`)

`05 enabled 01 lo hi`, value = hours * 60 + minutes. The original app collects "turn on / turn off" but never sends it, the third byte is always `01`.

## Mechanisms used by the app

### Write queue

Each lamp has one promise chain, so GATT writes never overlap. Sliders and the colour wheel go through a coalescing sender: only the newest packet per kind is sent while the previous one is in flight.

### Static segments

There is no per-LED command. LEDs past the IC length (`0F`) keep their last colour. The app uses that to paint fixed colour segments on Magic strips:

1. Set the full length and send the top colour.
2. Shrink the length to the next boundary and send the next colour.
3. Repeat down to the bottom segment.

Painting from the bottom up does not work. "Apply all" waits 3 s between segments, otherwise the lamp misses some of them. Each segment can also be applied on its own. The minimum length is 16 LEDs.

While segments are active, the first power, colour, mode, rhythm, motor or laser command first sends `0F` with the full length. Brightness does not reset segments.

### Phone microphone

Every 150 ms the app reads samples, computes `20 * log10(mean |sample|) * 2.55` (same as the original app), clamps it to 0-255 and sends `10` with the chosen colour or a rotating one. The lamp microphone mode (`04`) has no colour.

### Reconnecting

Lamps paired before are restored with `navigator.bluetooth.getDevices()` and connected without the chooser. If a lamp is off, the app waits for its advertisement (`watchAdvertisements`) and retries with backoff. A dropped connection is retried automatically unless the user pressed "Disconnect". In Chrome `getDevices()` may need `chrome://flags/#enable-web-bluetooth-new-permissions-backend`.

### Local storage

All keys start with `raingel.`: `settings` (brightness, channel order, IC length), `group.<deviceId>`, `alarms`, `delay`, `favorites`, `segments`, `segmentsActive`.

## References

- [agustinprod/vclights](https://github.com/agustinprod/vclights), the same protocol taken from the Raingel APK, with tests on real lamps ([protocol notes](https://github.com/agustinprod/vclights/blob/main/docs/protocol.md))
- [Seventh-Void/raingel-led-hass](https://github.com/Seventh-Void/raingel-led-hass), Home Assistant integration
- [AndrianBdn/open-vc-blelight](https://github.com/AndrianBdn/open-vc-blelight), Python control with bleak
- [arizustudio/vc-blelight-studio-pro](https://github.com/arizustudio/vc-blelight-studio-pro), desktop controller for VC-BLELIGHT lamps

## License

MIT, see [LICENSE](LICENSE).
