# DDJ-FLX10 USB settings protocol

Status: **static analysis, unverified on hardware.** Every item below was
read out of the vendor's macOS settings utility without a controller attached.
Treat all of it as a hypothesis until a USB capture confirms it (recipe at the
end).

This document is our own description of the protocol, written for
interoperability (SPEC §2, §9.3). No vendor code, binaries or resources are
copied into the repository. The implementation lives in
`crates/od-controller/src/flx10_settings.rs`.

## Evidence

Analysed files (arm64 slice of each universal binary, macOS):

| File | Role |
|---|---|
| `/Library/Frameworks/DDJ-FLX10Setup.framework/Versions/A/DDJ-FLX10Setup` | Settings library: all USB access |
| `/Library/Frameworks/DDJ-FLX10Setup.framework/Versions/A/Headers/PDJFunc.h` | Public API (function names and parameters) |
| `/Applications/Pioneer/DDJ-FLX10/DDJ-FLX10 Setup.app/Contents/MacOS/DDJ-FLX10 Setup` | UI: maps UI choices to the index values passed to the library |
| `…/DDJ-FLX10 Setup.app/Contents/Resources/English.lproj/Localizable.strings` | UI labels for those choices |

Method: disassembly (`objdump --macho -d`) plus Objective-C metadata
(`otool -oV`). The REA MCP session was opened but its Hopper provider timed out
during startup and Ghidra was not configured in the REA environment, so REA
was not used for the analysis.

Library structure: the public `PDJ_*` functions call one small helper per
setting (`SetFaderStartOptionSetting`, `SetJogRingColorSetting`, …). Each
helper calls `DeviceRequest`, which fills an `IOUSBDevRequest` and calls
`IOUSBDeviceInterface::DeviceRequest`, so every transfer goes to the
**default control pipe (endpoint 0)** with recipient = device. No interface is
claimed. Device lookup (`openUSB`, `CheckUSBAudio`) matches VID `0x2B73`,
PID `0x0041` and skips devices with class 9 (hubs). Setters loop over every
attached FLX10.

## Writes

All writes use `bmRequestType = 0x40` (vendor, host-to-device, device),
`bRequest = 0x03`, `wLength = 0` (no data stage). `wIndex` selects the
setting, `wValue` carries the value. For every setting except USB input
routing, `wValue = index << 8` (`PDJ_Set*Index` computes `(index << 8) & 0xFFFF`).

| Setting | wIndex | wValue | Values (UI label → index) | Evidence | Confidence |
|---|---|---|---|---|---|
| Fader start | `0xC013` | `idx << 8` | Off → 0, On + SYNC → 1, On → 2 | `PDJ_SetFaderStartIndex` → `SetFaderStartOptionSetting`; app `-[AudioSetup onCheckFaderSyncOn:]`, `executeSetValueCommand` | wire: high; meaning: medium |
| REC OUT | `0xC01D` | `idx << 8` | With mic → 0, Without mic → 1 | `PDJ_SetRecOutIndex` → `SetRecOutSetting`; app `setOutput:value:` (channel 3), pull-down order in `awakeFromNib` | wire: high; meaning: medium |
| Time mode | `0xC026` | `idx << 8` | Elapsed → 0, Remaining → 1 | `PDJ_SetTimeModeIndex` → `SetTimeDisplayModeSetting`; app `onChangeTimeDisplayFormat:` | wire: high; meaning: medium-high |
| Virtual deck mode | `0xC027` | `idx << 8` | Off → 0, On → 1 | `PDJ_SetVirtualDeckModeIndex` → `SetVirtualDeckModeSetting`; app `onCheckVirtualDeckMode:` | high |
| Slip mode flashing | `0xC028` | `idx << 8` | Disable → 0, Enable → 1 | `PDJ_SetSlipModeIndex` → `SetSlipModeFlashingSetting`; app `onChangeModeFlashing:` | wire: high; meaning: medium-high |
| Slip button LED | `0xC029` | `idx << 8` | Lit → 0, Blink → 1 | `PDJ_SetSlipModeButtonLitIndex` → `SetSlipModeButtonLitSetting`; app `onChangeSlipButtonSetting:` | wire: high; meaning: medium-high |
| Jog ring colour | `0xC02B` | `idx << 8` | MODE 1 → 0, MODE 2 → 1, MODE 3 → 2 | `PDJ_SetJogRingColorIndex` → `SetJogRingColorSetting`; app `setOutput:value:` (channel 2) | wire: high; colours behind each mode: unknown |
| USB output boost | `0x8003` | `idx << 8` (table) | +15 dB → 0 … 0 dB → 5 (steps of 3 dB) | `PDJ_SetBoostLevelIndex` → `SetUSBOutputLevelSetting`, value table in `__data` | high (wire), not exposed in the FLX10 app UI |
| USB input routing | `0x8002` | see below | see below | `PDJ_SetUSBInputAudio` → `SetMixerOutputSetting`, value table in `__data` | high (wire), not exposed in the FLX10 app UI |
| MIDI channel | `0x8004` | unknown | — | helper `SetMidiChannelSetting` exists but is never called; `PDJ_*MidiChannel*` are stubs returning "unsupported" | register: low; encoding: unknown |
| MIDI key mode | `0x8005` | unknown | — | helper `SetMidiKeyModeSetting`, same situation | register: low; encoding: unknown |

When the utility starts it replays the saved values, falling back to these
defaults (`executeSetValueCommand`): slip flashing 1, slip LED 0, fader start
2, jog ring 0, time mode 1, virtual deck 1, REC OUT 0. The utility stores the
fader start choice as 0/1 (off), 2 (on) or 3 (on + SYNC) in its preference
file and translates 3 → 1, 2 → 2, 0/1 → 0 when it sends it.

### USB input routing value

`PDJ_SetUSBInputAudio(channel, index)` looks up a 5 × 13 table of 16-bit
values. Channel 0 is the master/record stream (13 choices); channels 1–4 are
the per-channel streams (10 choices). The high byte is `channel + 1`; the low
byte is the source:

| Low byte | Source |
|---|---|
| `0x01` / `0x02` / `0x03` | channel input LINE / DIGITAL / PHONO (only on that channel's stream) |
| `0x05` / `0x06` | pre-fader / post-fader of that channel (streams 1–4) |
| `0x3n` / `0x1n` | pre-fader / post-fader of channel n (stream 0 only) |
| `0x07` / `0x08` | crossfader A / B |
| `0x09` | MIC |
| `0x0A` / `0x0E` | MIX (REC OUT) with / without mic |

Examples: stream 0 ← MIX with mic = `0x010A`; stream 0 ← pre-fader CH3 =
`0x0133`; stream 2 ← CH2 PHONO = `0x0303`.

`PDJ_SetUSBInputAudioDirect(channel, mixerChannel, source)` uses a second
encoding on the same register: `((mixerChannel + 1) << 12) | (((channel + 1)
& 0xF) << 8) | source`. The app never calls it. Not implemented.

After routing a stream, the library reads the input selector register (below)
and returns a warning if the stream was set to a channel input whose physical
selector is in one of the states {6, 7, 17, 18, 21, 22}. The meaning of those
states is not known.

## Reads

All reads use `bmRequestType = 0xC0`, `bRequest = 0x00`, `wValue = 0`.

| Item | wIndex | wLength | Decoding | Evidence | Confidence |
|---|---|---|---|---|---|
| Firmware version | `0xC001` | 2 | little-endian u16; shown as `"%x.%.2x"` of `(v >> 8, v & 0xFF)`, e.g. `0x0102` → "1.02" | `GetFirmwareVersionNumber`, `PDJ_GetFirmwareVersionDevice`; app `-[BackView drawRect:]` | high |
| Input selector state | `0x8002` | 5 | one byte per channel; byte `channel + 1` is read for USB channel `channel`, values 0–0x18 are valid | `GetInputSelectorInfo`, `PDJ_GetMixerInputSelectorStatus` | medium (indexing is inconsistent inside the library) |
| "Serato active" | `0x8004` | 2 | byte 0 is 0 or 1 | `GetSeratoActiveInfo`, `PDJ_GetMixerSeratoActiveStatus` | medium |
| Port info | `0x8006` | 2 | byte 0 used, meaning unknown | `GetPortInfo`, `PDJ_GetPortInfo` | low |
| Certification level | `0x8007` | 4 | big-endian u32 | `GetCertifyLevel`, `PDJ_GetMixerCertifyLevel` | medium |

## Host-only settings (no USB traffic)

These are stored in `~/Library/Preferences/com.pioneerdj.DDJ-FLX10autosetup.plist`
and never sent to the device by the library:

- Auto start-up (`PDJ_SetAutoStartUpIndex`, key `AutoStartUp`): whether the
  vendor launcher starts the utility.
- Software selection (`PDJ_SaveSoftwareIndex`, key `softwarePulldownIndex`;
  rekordbox / Serato DJ Pro / other).
- All `PDJ_Save*` and `PDJ_GetCurrent*` functions: they read and write this
  file only. The device cannot be queried for its current settings with any
  request we found.

## Not determined statically

- Whether the device keeps settings across power cycles. The utility replays
  them on every start, which suggests it does not.
- The colours behind jog ring MODE 1/2/3.
- MIDI channel / key mode encoding (registers `0x8004`/`0x8005` are guesses
  from unused helpers).
- What the port info and input selector bytes mean.
- Whether the device ACKs or STALLs unknown values (the library ignores errors
  on most writes).
- The "DJ LOGO DISPLAY" setting and jog display content. The settings
  library has no code for them, so they need a capture with the vendor
  software that drives them.

## Verifying on hardware

1. Note the firmware version from the vendor utility's About tab.
2. Capture USB while changing each setting in the vendor utility, one at a
   time.
   - **macOS**: install Wireshark, then
     `sudo ifconfig XHC20 up` (older Intel) or, on Apple Silicon / macOS 13+,
     `sudo ifconfig XHC0 up` / `XHC1` (try `ifconfig -l` and pick the `XHC*`
     interface that carries the controller). Capture on that interface with
     filter `usb.idVendor == 0x2b73 || usb.bmRequestType == 0x40 || usb.bmRequestType == 0xc0`.
     Afterwards `sudo ifconfig XHC0 down`. The library also has its own log
     line `Device Request:bRequest=…,wValue=…,wIndex=…,wLength=…` (via
     `NSLog`, visible in Console.app), but it only prints when its debug level
     is non-zero (`PDJ_SetFrameworkDebugLevel`). We found no UI switch for
     that, so the capture is the practical route.
   - **Linux** (vendor utility not available): load `usbmon`
     (`sudo modprobe usbmon`), find the bus with `lsusb -d 2b73:0041`,
     capture `usbmonN` in Wireshark, and send transfers with our code
     (`cargo run` an example using `Flx10Device`, feature `usb-settings`).
     Needs a udev rule granting access to `2b73:0041`.
3. For each setting check `bmRequestType`, `bRequest`, `wValue`, `wIndex`,
   `wLength` against the tables above, and watch the controller for the
   visible effect (jog ring colour, time display, slip LED…).
4. Read the firmware version with `Flx10Device::firmware_version()` and
   compare it with step 1.
5. Update the confidence columns and remove "unverified" for confirmed rows.
