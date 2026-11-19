# smbc-ruffle: Super Mario Bros. Crossover as a native game

This branch (`smbc-native`) is a fork of [Ruffle](https://github.com/ruffle-rs/ruffle)
that runs Exploding Rabbit's Flash game *Super Mario Bros. Crossover 3.1.21* as a
native Windows program. Linux is planned.

The game itself is **not** in this repository. You need your own copy of the game
file, placed next to the exe as `game\smbc3.swf`.

## What this fork adds

- **Native music:** the game's music engine (an Alchemy build of game-music-emu) is
  replaced with native game-music-emu (`smbc_gme/`, LGPL). The game's calls to it
  are intercepted in `core/src/avm2/smbc_gme.rs`. Also adds support for sound
  generated through `SampleDataEvent`.
- **Widescreen:** the stage width follows the monitor the game starts on (16:9,
  21:9), rounded to a multiple of 32. HUD, score pops, end-of-level text, water
  and screen edges are fixed up for it in `core/src/avm2/smbc_wide.rs`.
- **In-game saves:** the save and load file dialogs are replaced with five save
  slots plus an autosave after each finished level (`<data>\saves\slots`).
- **Global settings:** options, keys, controller buttons, enabled
  characters/skins and unlocked cheats are kept across launches instead of per
  save.
- **Controllers:** gamepad support, plus an in-game Options > Controller menu
  (`core/src/input.rs`).
- **Launcher mode** (`desktop/src/smbc.rs`): starts the bundled game directly,
  remembers fullscreen, F11 / Alt+Enter toggle fullscreen.
- **Fixes:** Samus explosion halves, random character skin, map skin choice
  between stages, and the end of SNES songs being cut off.

Some features (the save slot menus, controller menu, map skin menu) also need
ActionScript additions compiled into the game's SWF. Those aren't in this
repository because they patch the game itself.

## Building (Windows exe, cross-compiled from Linux)

You need Rust nightly features (`-Zbuild-std`) and
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw) (UCRT build) on `PATH`:

```sh
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
export CC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang
export CXX_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-clang++
export AR_x86_64_pc_windows_gnullvm=llvm-ar
export RC_x86_64_pc_windows_gnullvm=x86_64-w64-mingw32-windres
export RUSTC_BOOTSTRAP=1
cargo build -p ruffle_desktop --release --target x86_64-pc-windows-gnullvm \
  -Zbuild-std=std,panic_abort --no-default-features --features software_video,lzma
```

The exe is `target/x86_64-pc-windows-gnullvm/release/ruffle_desktop.exe`.
Rename it as you like, and put the game at `game\smbc3.swf` beside it.

`smbc_bench/` is a headless test runner used during development. It scripts key
presses and takes screenshots.

## Licenses

Ruffle is MIT / Apache-2.0 (see `LICENSE.md`). game-music-emu in `smbc_gme/gme`
is LGPL 2.1 (see `smbc_gme/LICENSE.md`). *Super Mario Bros. Crossover* is by
Exploding Rabbit; its characters belong to Nintendo and their other owners. This
project isn't affiliated with any of them.
