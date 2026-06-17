# comeow — a human-friendly CANopen SDO client

A small, cross-platform CANopen **SDO client** for interactive debugging and
scripted read/write sequences. Unlike a full CANopen node, it does **not** run
a stack and does **not** need its own node-id — it just talks SDO to whatever
node you point it at, and passively prints EMCY messages from anyone on the bus.

Built on [`can-transport`](https://crates.io/crates/can-transport) and
[`canopen-sdo`](https://crates.io/crates/canopen-sdo).

## Why

The full-stack alternatives (e.g. CANopenLinux) force you to bring up a whole
node with its own id, bind to SocketCAN, and offer only a bare stdio prompt
with no history. This tool is the opposite: a focused client with a real line
editor (history, completion), a script mode, and a pluggable transport.

## Features

- Interactive shell with line editing, **up/down history**, and **context-aware
  tab completion** (commands → verb → common object indexes with names →
  datatypes, depending on where the cursor is).
- **Live validation** (fish-style), updated on every keystroke, distinguishing
  *wrong* from *still-typing*:
  - **green** — a complete, valid command;
  - **red** — a token is wrong (e.g. `1 r 0x1017 o`, a bad index, or a write
    value out of range) — it can't become valid no matter what you type next;
  - **normal** — a blank line or a valid prefix you're still typing
    (`r 0x1017`, `u1` on the way to `u16`).
- **Script mode** (`-f file`): run read/write/`sleep` lines in sequence.
- Live **EMCY** decoding from every node, printed without disrupting the prompt.
- CiA 309-style commands and datatypes you already know.
- Pluggable backend: **SocketCAN** (Linux, default) or **gs_usb**
  (candleLight USB, cross-platform — Windows/macOS/Linux).

## Build

```sh
cargo build --release                              # socketcan (default)
cargo build --release --no-default-features --features gs_usb   # USB adapter
cargo build --release --features "socketcan gs_usb"             # both
```

## Usage

```sh
# interactive, SocketCAN (the default backend). Bring the iface up first:
#   ip link set up can0 type can bitrate 1000000
comeow                      # uses can0 by default
comeow can0 --node 0x10     # explicit interface
comeow can1                 # a different interface

# interactive, gs_usb adapter (classic 1 Mbit; add --fd for 1M/5M CAN-FD)
comeow --gs-usb

# run a script and exit non-zero on any error
comeow can0 -f session.scr --stop-on-error
```

The first positional argument is the SocketCAN interface (default `can0`).
Key flags: `--gs-usb` (USB backend), `--node <id>` (default node),
`--timeout <ms>`, `--retries <n>`, `--no-emcy` (disable the EMCY listener),
`--no-highlight` (disable live input colouring — see [TODO.md](TODO.md) re: a
possible CJK rendering glitch), `-f <file>` (script mode).

## Commands

```text
[<node>] r[ead]  <index> <sub> [<datatype>]    read an object
[<node>] w[rite] <index> <sub> <datatype> <v>  write an object
set node <id>            set the default node (then omit <node>)
set timeout <ms>         per-operation SDO timeout
set retries <n>          retry count on timeout / I/O error
sleep <ms>               pause (handy in scripts)
help [datatype]          show help
quit | exit | q          leave
```

- Indexes / sub-indexes accept `0x..` hex or decimal.
- `#` starts a comment (to end of line) in scripts.

### Datatypes (CiA 309 tokens)

`b` · `u8 u16 u32 u64` · `i8 i16 i32 i64` · `x8 x16 x32 x64` (hex display) ·
`r32 r64` (float) · `vs` (visible string) · `hex` (raw space-separated bytes).
On **read**, the datatype is optional — without it the raw bytes are shown as hex.

### Example session

```text
canopen> set node 0x10
default node = 16 (0x10)
canopen> r 0x1018 1 u32
0x1018:01 = 1230
canopen> w 0x1017 0 u16 500
OK
canopen> r 0x6041 0
[EMCY] node 0x05: code 0x3120 (voltage) | reg 0x04 [voltage] | vendor [AA, BB, 00, 00, 00]
0x6041:00 = 2 bytes: 37 06  (u16=0x637, 1591)
```

## Testing without hardware

A mock node is included. On Linux with a virtual CAN interface:

```sh
sudo modprobe vcan
sudo ip link add dev vcan0 type vcan && sudo ip link set up vcan0
cargo run --example mock_node -- vcan0 5         # in one terminal
comeow vcan0 -f examples/test.scr    # in another
```

## Notes / limitations

- **Block transfer is not implemented** (segmented + expedited are). Block mode
  is rarely needed for debugging.
- **gs_usb bitrate** currently has only fixed presets (classic 1 Mbit, or
  1M/5M CAN-FD). Arbitrary rates (250k/500k/…) will be added later in
  `can-transport`. SocketCAN takes its bitrate from the OS, so it is unaffected.
- gs_usb on Linux needs usbfs access (it detaches the kernel driver) — run with
  `sudo` or add a udev rule.
- `Cargo.toml` depends on the published `can-transport` / `canopen-sdo` crates.
  For local development against sibling checkouts, a git-ignored
  `.cargo/config.toml` adds `paths = ["../can-transport", "../canopen-sdo"]`,
  so local builds transparently use your working copies while CI uses crates.io.

## Releases / CI

[`.github/workflows/release.yml`](.github/workflows/release.yml) builds static
musl binaries with [`cross`](https://github.com/cross-rs/cross):

- push / PR / manual run → builds `x86_64` and `aarch64` musl binaries and
  uploads them as run artifacts;
- pushing a tag `v*` (e.g. `git tag v0.1.0 && git push --tags`) additionally
  creates a **draft** GitHub Release with both `.tar.gz` archives attached.
