//! Packaging for Super Mario Bros. Crossover.
//!
//! When `game/smbc3.swf` sits next to the executable, Ruffle behaves as a
//! dedicated game executable: the movie opens immediately, the menu bar is
//! hidden, the window fits the game, and gamepads are mapped to the game's
//! keyboard controls. Extra command-line style options can be put (one per
//! line) in `options.txt` in the data folder or next to the executable.

use std::ffi::OsString;
use std::path::PathBuf;

pub const TITLE: &str = "Super Mario Bros. Crossover";

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|p| p.to_path_buf())
}

pub fn bundled_swf() -> Option<PathBuf> {
    let path = exe_dir()?.join("game").join("smbc3.swf");
    path.is_file().then_some(path)
}

pub fn is_bundle() -> bool {
    bundled_swf().is_some()
}

/// Where saves, settings and logs live. A file named `portable` (or
/// `portable.txt`) next to the executable keeps everything beside it.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = exe_dir()
        && (dir.join("portable").exists() || dir.join("portable.txt").exists())
    {
        return dir.join("UserData");
    }
    dirs::data_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SMBC3")
}

/// Default controller layout (Xbox naming):
/// A = jump (Z), X / Y = attack (X), B / RB = special (C),
/// Start = pause (Enter), Back = select (A), D-pad = arrows.
/// The left stick is turned into D-pad presses in `app.rs`.
const DEFAULT_PAD: &[&str] = &[
    "south=z",
    "west=x",
    "north=x",
    "east=c",
    "right-trigger=c",
    "start=return",
    "select=a",
    "d-pad-up=up",
    "d-pad-down=down",
    "d-pad-left=left",
    "d-pad-right=right",
];

fn read_option_lines(path: PathBuf) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Builds the argument list Ruffle is started with.
pub fn build_args() -> Vec<OsString> {
    let mut os_args: Vec<OsString> = std::env::args_os().collect();
    let Some(swf) = bundled_swf() else {
        return os_args;
    };
    let exe = if os_args.is_empty() {
        OsString::from("ruffle")
    } else {
        os_args.remove(0)
    };

    let data = data_dir();
    let _ = std::fs::create_dir_all(&data);
    write_default_options(&data);

    // Options from files first, then the real command line (which wins).
    let mut extra: Vec<OsString> = Vec::new();
    if let Some(dir) = exe_dir() {
        extra.extend(read_option_lines(dir.join("options.txt")).into_iter().map(OsString::from));
    }
    extra.extend(read_option_lines(data.join("options.txt")).into_iter().map(OsString::from));
    extra.extend(os_args);

    let given: Vec<String> = extra
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let has = |flags: &[&str]| {
        given.iter().any(|a| {
            flags
                .iter()
                .any(|f| a == f || a.starts_with(&format!("{f}=")))
        })
    };

    let mut args: Vec<OsString> = vec![exe];
    let mut push = |s: String| args.push(OsString::from(s));

    if !has(&["--show-gui"]) {
        push("--no-gui".into());
    }
    if !has(&["--no-force-scale"]) {
        push("--force-scale".into());
    }
    if !has(&["--scale", "-s"]) {
        push("--scale=show-all".into());
    }
    if !has(&["--letterbox"]) {
        push("--letterbox=on".into());
    }
    if !has(&["--width"]) && !has(&["--height"]) {
        push("--width=1024".into());
        push("--height=960".into());
    }
    if !has(&["--max-execution-duration"]) {
        // Start-up pre-renders every sound effect through the game's music
        // emulator in one go; never treat that as a hung script.
        push("--max-execution-duration=3600".into());
    }
    if !has(&["--save-directory"]) {
        push(format!("--save-directory={}", data.join("saves").display()));
    }
    if !has(&["--config"]) {
        push(format!("--config={}", data.join("config").display()));
    }
    if !has(&["--cache-directory"]) {
        push(format!("--cache-directory={}", data.join("cache").display()));
    }
    if !has(&["--gamepad-button", "-B"]) {
        for m in DEFAULT_PAD {
            push(format!("--gamepad-button={m}"));
        }
    }

    let has_movie = given
        .iter()
        .any(|a| !a.starts_with('-') && a.to_ascii_lowercase().ends_with(".swf"));
    // Filter out our own pseudo-flags.
    args.extend(
        extra
            .into_iter()
            .filter(|a| a != "--show-gui" && a != "--no-force-scale"),
    );
    if !has_movie {
        args.push(swf.into_os_string());
    }
    args
}

fn write_default_options(data: &std::path::Path) {
    let path = data.join("options.txt");
    if path.exists() {
        return;
    }
    let text = "\
# Super Mario Bros. Crossover - launcher options
# One option per line; lines starting with # are ignored.
# Remove the # in front of a line to use it.
#
# Start in fullscreen:
#--fullscreen
#
# Show Ruffle's menu bar:
#--show-gui
#
# Window size:
#--width=1536
#--height=1440
#
# Controller mapping. If ANY --gamepad-button line is active, the built-in
# mapping is replaced, so list every button you want.
# Buttons: south (A/Cross), east (B/Circle), west (X/Square), north (Y/Triangle),
#          left-trigger (LB), right-trigger (RB), left-trigger2 (LT),
#          right-trigger2 (RT), select (Back), start, d-pad-up/down/left/right
# Keys must match the game's own Options > Controls (defaults shown).
#--gamepad-button=south=z
#--gamepad-button=west=x
#--gamepad-button=north=x
#--gamepad-button=east=c
#--gamepad-button=right-trigger=c
#--gamepad-button=start=return
#--gamepad-button=select=a
#--gamepad-button=d-pad-up=up
#--gamepad-button=d-pad-down=down
#--gamepad-button=d-pad-left=left
#--gamepad-button=d-pad-right=right
#
# Graphics backend (if you have rendering problems): vulkan, dx12, gl
#--graphics=dx12
";
    let _ = std::fs::write(path, text);
}

/// Lightweight frame-time statistics written to the log, so slow stages can be
/// diagnosed from `ruffle.log` after a play session.
pub struct Perf {
    window_start: std::time::Instant,
    tick_ms: Vec<f32>,
    render_ms: Vec<f32>,
    present_ms: Vec<f32>,
}

impl Default for Perf {
    fn default() -> Self {
        Self {
            window_start: std::time::Instant::now(),
            tick_ms: Vec::with_capacity(1024),
            render_ms: Vec::with_capacity(1024),
            present_ms: Vec::with_capacity(1024),
        }
    }
}

impl Perf {
    pub fn tick(&mut self, ms: f32) {
        if ms > 50.0 {
            tracing::info!(target: "smbc_perf", "slow update: {ms:.1} ms");
        }
        self.tick_ms.push(ms);
        self.maybe_report();
    }
    pub fn render(&mut self, ms: f32) {
        if ms > 50.0 {
            tracing::info!(target: "smbc_perf", "slow render: {ms:.1} ms");
        }
        self.render_ms.push(ms);
    }
    pub fn present(&mut self, ms: f32) {
        self.present_ms.push(ms);
    }
    fn maybe_report(&mut self) {
        let elapsed = self.window_start.elapsed().as_secs_f32();
        if elapsed < 10.0 {
            return;
        }
        fn stats(v: &mut [f32]) -> String {
            if v.is_empty() {
                return "-".into();
            }
            v.sort_by(|a, b| a.total_cmp(b));
            let avg = v.iter().sum::<f32>() / v.len() as f32;
            format!(
                "avg {avg:.1} p95 {:.1} max {:.1}",
                v[(v.len() * 95 / 100).min(v.len() - 1)],
                v[v.len() - 1]
            )
        }
        use std::sync::atomic::Ordering::Relaxed;
        let syncs = ruffle_core::SMBC_GPU_SYNCS.swap(0, Relaxed);
        let draws = ruffle_core::SMBC_OFFSCREEN_DRAWS.swap(0, Relaxed);
        tracing::info!(
            target: "smbc_perf",
            "{:.0} presents/s | update ms: {} | render ms: {} | present ms: {} | gpu readbacks/s {:.1} | offscreen draws/s {:.1}",
            self.present_ms.len() as f32 / elapsed,
            stats(&mut self.tick_ms),
            stats(&mut self.render_ms),
            stats(&mut self.present_ms),
            syncs as f32 / elapsed,
            draws as f32 / elapsed
        );
        *self = Self::default();
    }
}
