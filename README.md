# zx81-rust

A local ZX81 emulator with its machine, memory, ULA/video, keyboard, and file
integration implemented in Rust. The CPU uses the Rust `z80emu` library.
The executable is **`zx81-rust`**.

The Sinclair edition 3 ROM is embedded in the executable. Building and running
are independent of OpenSTD and eighty-three: no source code, library, or worker
from either project is used. The original firmware notices are in
[`roms/NOTICE.txt`](roms/NOTICE.txt); the copy's provenance is recorded in
[`roms/ORIGIN.txt`](roms/ORIGIN.txt).

## Build and run

Validated on Linux with Rust/Cargo 1.97.1. The window uses X11, or XWayland in
a Wayland session. Headless mode works without a display server. Cargo downloads
dependencies during the first build.

```bash
cargo build --release --locked
./target/release/zx81-rust
./target/release/zx81-rust examples/hello.p
```

Without an argument, the emulator opens BASIC with the **K** cursor. With a `.p`
file, it loads the program and waits for `RUN`: press **R**, then **Enter**.
`examples/hello.p` was created in this emulator's BASIC and prints `HI`.

You can copy the executable to another directory or a compatible Linux computer.
The ROM is embedded at build time, so the `roms` directory is not needed at
runtime. The current build targets **x86-64**, uses **glibc 2.34 or later** and
`libgcc_s`, and loads the **X11 and Xcursor** system libraries for its window.
It requires an X11 or XWayland session to display the window.

## Keyboard

The physical keyboard maps to the ZX81's 40 keys. The original BASIC interprets
keys according to its K, L, F, or G cursor mode. At the K cursor, pressing **P**
inserts the entire `PRINT` keyword.

| ZX81 command or key | Computer keyboard |
| --- | --- |
| PRINT / RUN / SAVE / LOAD | P / R / S / J at the K cursor |
| LIST / NEW | K / A at the K cursor |
| Quotes `"` | Shift + P, or the apostrophe key |
| Delete | Backspace or Shift + 0 |
| Cursor left/down/up/right | Arrow keys or Shift + 5/6/7/8 |
| BREAK | Space or Escape |
| FUNCTION | Shift + Enter |
| Show keyboard help | F1 |
| Pause/resume | F5 |
| Reset to BASIC | F9 |
| Quit | Ctrl + Q, or close the window |

To enter `10 PRINT "HELLO"`:

1. Press **1**, **0**, **P**.
2. Press **Shift + P**, **H**, **E**, **L**, **L**, **O**, **Shift + P**.
3. Press **Enter** and wait for BASIC to update the listing.
4. Press **R**, **Enter** to run the program.

Keys are released when the window loses focus. The help screen suspends emulation;
F1 returns to the machine. F9 clears RAM and returns to BASIC.

## Save and load programs

BASIC file commands use the **working directory from which the executable was
started**:

```basic
SAVE "TEST"
LOAD "TEST"
```

`SAVE "TEST"` writes `TEST.p`. At the K cursor, press **S**, **Shift + P**, the
letters of the name, **Shift + P**, and **Enter**. A save notification appears
in the window and terminal.

The `.p` extension is appended when needed. The ZX81 uses uppercase letters,
so typing `SAVE "test"` on its keyboard produces `TEST.p`. `LOAD` looks up file
names without distinguishing uppercase and lowercase. Names are local file
names, without directory paths.

Saving again with the same name replaces the file with a complete image.
Read or write failures display a message and return control to BASIC.
A failed read preserves the program in memory.

A `.p` file contains the native memory image from VERSN (`$4009`) through
`E_LINE-1`, including the program, variables, and display. Loading prepares the
editing workspace again and leaves the program ready for `RUN`, even if it was
saved during execution. `LOAD ""` reloads the last program opened from a file;
without an associated program, it reports a recoverable error.

## Settings and diagnostics

The default is a PAL ZX81 with 16 KiB of RAM. Configurations with 1, 2, 4, and
8 KiB are also available. CPU fetch cycles and the ULA produce the video, with
interrupts, wait states, and FAST/SLOW modes. The displayed crop is 384x288,
scaled locally while preserving its aspect ratio.

```bash
./target/release/zx81-rust --ram 1
./target/release/zx81-rust --wrx program.p
./target/release/zx81-rust --headless --frames 300 --screenshot screen.ppm
./target/release/zx81-rust --help
```

`--wrx` enables RAM pixel fetches using the refresh address. The stock ZX81 is
silent. AY/ZonX extensions, ZXpand, printers, audio tape emulation, and save states
are outside this version's scope. File commands integrate the ROM's `SAVE`/`LOAD`
routines with `.p` files.

Headless mode initializes BASIC or loads the program, then executes the specified
number of 20 ms frames without waiting for the real-time clock. `--screenshot`
writes a PPM image; `--frames` can also limit a windowed session.

## Verification

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

Tests execute the actual ROM: booting all five RAM configurations, keyboard matrix
input, entering a program, `SAVE`, `NEW`, `LOAD`, startup loading and `RUN`, file
replacement, and error recovery. Additional tests cover the raster, memory map,
ROM glyphs, inverse video, WRX fetches, FAST/SLOW transitions, `.p` validation,
and command-line arguments.

Process tests copy the executable into an empty temporary directory and verify
boot and video output without external ROM files. They also load the `.p` demo
in that environment and check that invalid files are rejected. The X11 window
has been checked with typing, saving, running, keyboard help, and pause.
Compatibility with individual games and extensions requires specific validation.

## Project layout

| File | Responsibility |
| --- | --- |
| `src/machine.rs` | Rust CPU, bus, clock, and ROM file-routine integration. |
| `src/video.rs` | ULA, raster, shift register, synchronization, and PPM capture. |
| `src/keyboard.rs` | ZX81 keyboard matrix. |
| `src/program.rs` | `.p` validation and Sinclair file-name conversion. |
| `src/ui.rs` | Window, physical input, keyboard help, and pause. |
| `src/main.rs` | Executable and command-line arguments. |
| `tests/cli.rs` | Isolated executable verification. |

Direct dependencies and their notices are listed in
[`THIRD_PARTY_NOTICES.txt`](THIRD_PARTY_NOTICES.txt). `Cargo.lock` pins the
versions used by the build.
