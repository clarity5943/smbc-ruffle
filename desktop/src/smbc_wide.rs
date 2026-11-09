//! Widescreen version of the SMBC movie.
//!
//! Super Mario Bros. Crossover is built for a 512x480 stage. This rewrites the
//! movie for a stage of `W`x480 (W picked from the monitor's aspect ratio):
//!
//! * the SWF header's stage width;
//! * `ScreenSize.SCREEN_WIDTH` (set in its static initialiser), which
//!   `GlobVars.STAGE_WIDTH` and `Level.GLOB_STG_RHT` are derived from;
//! * default values of a few slots: the level's screen width and scroll
//!   positions, and the default x position message boxes centre on;
//! * `CharacterSelect.PLAYER_X_POS`, which must stay relative to its 512 px map.
//!
//! Ruffle's `smbc_wide` hooks (core) handle the rest at run time (HUD position,
//! narrow maps, skin previews). The result is cached in the data folder.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const ORIGINAL_WIDTH: u32 = 512;
const HEIGHT: u32 = 480;

/// Stage width for a display of the given size: 480 px high, same aspect
/// ratio, rounded to a multiple of 16 (the game's tile size).
pub fn width_for_display(w: u32, h: u32) -> u32 {
    if w == 0 || h == 0 {
        return ORIGINAL_WIDTH;
    }
    let exact = HEIGHT as f64 * w as f64 / h as f64;
    let rounded = ((exact / 16.0).round() * 16.0) as u32;
    rounded.clamp(ORIGINAL_WIDTH, 2048)
}

/// Size of the primary monitor, if it can be found.
#[cfg(windows)]
pub fn primary_display_size() -> Option<(u32, u32)> {
    // The monitor the game window will open on: Windows puts a new window on
    // the monitor of the window in front when it starts (the Explorer
    // window that launched it), so use that one; else the one under the
    // mouse; else the primary monitor.
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONULL,
        MONITOR_DEFAULTTOPRIMARY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};
    unsafe {
        let fg = GetForegroundWindow();
        let mut mon = if fg.is_null() { std::ptr::null_mut() } else { MonitorFromWindow(fg, MONITOR_DEFAULTTONULL) };
        if mon.is_null() {
            let mut pt = POINT { x: 0, y: 0 };
            GetCursorPos(&mut pt);
            mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if !mon.is_null() && GetMonitorInfoW(mon, &mut info) != 0 {
            let w = (info.rcMonitor.right - info.rcMonitor.left) as u32;
            let h = (info.rcMonitor.bottom - info.rcMonitor.top) as u32;
            if w > 0 && h > 0 {
                tracing::info!("widescreen: launch monitor is {w}x{h}");
                return Some((w, h));
            }
        }
    }
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

#[cfg(not(windows))]
pub fn primary_display_size() -> Option<(u32, u32)> {
    None
}

/// Returns the path of the widened movie, creating it if needed.
pub fn widened_movie(swf: &Path, cache_dir: &Path, width: u32) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(swf).map_err(|e| e.to_string())?;
    let out = cache_dir.join(format!("smbc3_w{width}_{}_v2.swf", meta.len()));
    let _ = std::fs::create_dir_all(cache_dir);
    // The movie loads its data file relative to itself.
    if let Some(dir) = swf.parent() {
        let data_file = dir.join("smbc_data.txt.gz");
        if data_file.is_file() {
            let _ = std::fs::copy(&data_file, cache_dir.join("smbc_data.txt.gz"));
        }
    }
    if out.is_file() {
        return Ok(out);
    }
    let data = std::fs::read(swf).map_err(|e| e.to_string())?;
    let patched = patch(&data, width)?;
    // Remove stale versions.
    if let Ok(dir) = std::fs::read_dir(cache_dir) {
        for e in dir.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with("smbc3_w") && (n.ends_with(".swf") || n.ends_with(".tmp")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, patched).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &out).map_err(|e| e.to_string())?;
    Ok(out)
}

// ---------------------------------------------------------------------------

struct R<'a> {
    b: &'a [u8],
    p: usize,
}

impl<'a> R<'a> {
    fn new(b: &'a [u8], p: usize) -> Self {
        Self { b, p }
    }
    fn u8(&mut self) -> Result<u8, String> {
        let v = *self.b.get(self.p).ok_or("unexpected end of ABC")?;
        self.p += 1;
        Ok(v)
    }
    fn u30(&mut self) -> Result<u32, String> {
        let mut v: u32 = 0;
        for i in 0..5 {
            let c = self.u8()?;
            v |= ((c & 0x7f) as u32) << (7 * i);
            if c & 0x80 == 0 {
                break;
            }
        }
        Ok(v)
    }
    fn skip(&mut self, n: usize) {
        self.p += n;
    }
}

fn enc_u30(mut v: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let c = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            out.push(c | 0x80);
        } else {
            out.push(c);
            return out;
        }
    }
}

#[derive(Clone)]
struct Mn {
    qname: bool,
    public: bool,
    name: String,
}

struct Pool {
    ints: Vec<i32>,
    ints_count_at: (usize, usize),
    ints_end: usize,
    doubles: Vec<f64>,
    dbl_count_at: (usize, usize),
    dbl_end: usize,
    mns: Vec<Option<Mn>>,
    end: usize,
}

fn read_pool(abc: &[u8]) -> Result<Pool, String> {
    let mut r = R::new(abc, 4);
    let a = r.p;
    let n = r.u30()? as usize;
    let ints_count_at = (a, r.p);
    let mut ints = vec![0];
    for _ in 1..n.max(1) {
        ints.push(r.u30()? as i32);
    }
    let ints_end = r.p;
    let n = r.u30()? as usize;
    for _ in 1..n.max(1) {
        r.u30()?;
    }
    let a = r.p;
    let n = r.u30()? as usize;
    let dbl_count_at = (a, r.p);
    let mut doubles = vec![0.0];
    for _ in 1..n.max(1) {
        let bytes: [u8; 8] = abc
            .get(r.p..r.p + 8)
            .ok_or("bad double pool")?
            .try_into()
            .unwrap();
        doubles.push(f64::from_le_bytes(bytes));
        r.skip(8);
    }
    let dbl_end = r.p;
    let n = r.u30()? as usize;
    let mut strings = vec![String::new()];
    for _ in 1..n.max(1) {
        let l = r.u30()? as usize;
        strings.push(String::from_utf8_lossy(abc.get(r.p..r.p + l).ok_or("bad string")?).into_owned());
        r.skip(l);
    }
    let n = r.u30()? as usize;
    let mut nss: Vec<(u8, String)> = vec![(0, String::new())];
    for _ in 1..n.max(1) {
        let k = r.u8()?;
        let nm = r.u30()? as usize;
        nss.push((k, strings.get(nm).cloned().unwrap_or_default()));
    }
    let n = r.u30()? as usize;
    for _ in 1..n.max(1) {
        let c = r.u30()?;
        for _ in 0..c {
            r.u30()?;
        }
    }
    let n = r.u30()? as usize;
    let mut mns = vec![None];
    for _ in 1..n.max(1) {
        let k = r.u8()?;
        match k {
            0x07 | 0x0D => {
                let ns = r.u30()? as usize;
                let nm = r.u30()? as usize;
                let (nk, nsname) = nss.get(ns).cloned().unwrap_or_default();
                mns.push(Some(Mn {
                    qname: true,
                    public: nk == 0x16 && nsname.is_empty(),
                    name: strings.get(nm).cloned().unwrap_or_default(),
                }));
            }
            0x0F | 0x10 => {
                r.u30()?;
                mns.push(None);
            }
            0x11 | 0x12 => mns.push(None),
            0x09 | 0x0E => {
                let nm = r.u30()? as usize;
                r.u30()?;
                mns.push(Some(Mn {
                    qname: false,
                    public: false,
                    name: strings.get(nm).cloned().unwrap_or_default(),
                }));
            }
            0x1B | 0x1C => {
                r.u30()?;
                mns.push(None);
            }
            0x1D => {
                r.u30()?;
                let c = r.u30()?;
                for _ in 0..c {
                    r.u30()?;
                }
                mns.push(None);
            }
            _ => return Err(format!("bad multiname kind {k:#x}")),
        }
    }
    Ok(Pool {
        ints,
        ints_count_at,
        ints_end,
        doubles,
        dbl_count_at,
        dbl_end,
        mns,
        end: r.p,
    })
}

struct SlotTrait {
    owner: String,
    name: String,
    vindex: u32,
    vkind: u8,
    vpos: (usize, usize),
}

fn parse_traits(r: &mut R, pool: &Pool, owner: &str, out: &mut Vec<SlotTrait>) -> Result<(), String> {
    let n = r.u30()?;
    for _ in 0..n {
        let name = r.u30()? as usize;
        let kind = r.u8()?;
        match kind & 0x0f {
            0 | 6 => {
                r.u30()?;
                r.u30()?;
                let s = r.p;
                let vi = r.u30()?;
                let e = r.p;
                let vk = if vi != 0 { r.u8()? } else { 0 };
                out.push(SlotTrait {
                    owner: owner.to_owned(),
                    name: pool.mns.get(name).cloned().flatten().map(|m| m.name).unwrap_or_default(),
                    vindex: vi,
                    vkind: vk,
                    vpos: (s, e),
                });
            }
            1..=5 => {
                r.u30()?;
                r.u30()?;
            }
            k => return Err(format!("bad trait kind {k}")),
        }
        if kind & 0x40 != 0 {
            let c = r.u30()?;
            for _ in 0..c {
                r.u30()?;
            }
        }
    }
    Ok(())
}

fn slot_traits(abc: &[u8], pool: &Pool) -> Result<Vec<SlotTrait>, String> {
    let mut r = R::new(abc, pool.end);
    let n = r.u30()?;
    for _ in 0..n {
        let pc = r.u30()?;
        r.u30()?;
        for _ in 0..pc {
            r.u30()?;
        }
        r.u30()?;
        let fl = r.u8()?;
        if fl & 0x08 != 0 {
            let c = r.u30()?;
            for _ in 0..c {
                r.u30()?;
                r.u8()?;
            }
        }
        if fl & 0x80 != 0 {
            for _ in 0..pc {
                r.u30()?;
            }
        }
    }
    let n = r.u30()?;
    for _ in 0..n {
        r.u30()?;
        let c = r.u30()?;
        for _ in 0..c {
            r.u30()?;
            r.u30()?;
        }
    }
    let nc = r.u30()? as usize;
    let mut out = Vec::new();
    let mut names = Vec::with_capacity(nc);
    for _ in 0..nc {
        let nm = r.u30()? as usize;
        r.u30()?;
        let fl = r.u8()?;
        if fl & 0x08 != 0 {
            r.u30()?;
        }
        let ic = r.u30()?;
        for _ in 0..ic {
            r.u30()?;
        }
        r.u30()?;
        let name = pool.mns.get(nm).cloned().flatten().map(|m| m.name).unwrap_or_default();
        parse_traits(&mut r, pool, &name, &mut out)?;
        names.push(name);
    }
    for name in names.iter() {
        r.u30()?;
        parse_traits(&mut r, pool, name, &mut out)?;
    }
    Ok(out)
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    (from..=hay.len() - needle.len()).find(|&i| &hay[i..i + needle.len()] == needle)
}

/// Patches the game's main ABC block for a `w`-wide screen.
fn patch_abc(abc: &[u8], w: u32) -> Result<Vec<u8>, String> {
    let pool = read_pool(abc)?;
    let traits = slot_traits(abc, &pool)?;
    let half = w / 2;
    let more = |n: u32| more_shots(n, w);
    let want: &[(&str, &str, u32)] = &[
        // Player projectile limits (instance constants), scaled with the screen.
        ("MarioBase", "MAX_FIREBALLS_ON_SCREEN", more(2)),
        ("Samus", "MAX_SHOTS_ON_SCREEN", more(3)),
        ("Bill", "MAX_NORMAL_BULLETS", more(4)),
        ("Bill", "MAX_MACHINE_GUN_BULLETS", more(6)),
        // The spread gun fires 5 bullets at a time.
        ("Bill", "MAX_SPREAD_BULLETS", more(2) * 5),
        ("Level", "SCREEN_WIDTH", w),
        ("Level", "locStgRht", w),
        ("Level", "GLOB_STG_RHT", w),
        ("Level", "SCROLL_EDGE_DIST", half),
        ("Level", "DEFAULT_LEFT_SCROLL_POS", half),
        ("Level", "DEFAULT_RIGHT_SCROLL_POS", half),
        ("Level", "DEFAULT_RIGHT_SCROLL_POS_NO_LEFT_SCROLL", half - 16),
        ("GlobVars", "STAGE_WIDTH", w),
        ("ScreenSize", "SCREEN_WIDTH", w),
        ("LevObj", "GLOB_STG_RHT", w),
        ("MessageBox", "endXPos", half),
    ];
    let mut new_ints: Vec<i32> = Vec::new();
    let mut new_dbls: Vec<f64> = Vec::new();
    let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
    let mut hits = 0;
    for t in &traits {
        let Some(&(_, _, v)) = want.iter().find(|(c, n, _)| *c == t.owner && *n == t.name) else {
            continue;
        };
        let idx = match t.vkind {
            0x03 => {
                new_ints.push(v as i32);
                pool.ints.len() + new_ints.len() - 1
            }
            0x06 => {
                new_dbls.push(v as f64);
                pool.doubles.len() + new_dbls.len() - 1
            }
            _ => continue,
        };
        let _ = t.vindex;
        edits.push((t.vpos.0, t.vpos.1, enc_u30(idx as u32)));
        hits += 1;
    }
    if hits < 8 {
        return Err(format!("unexpected movie: only {hits} screen constants found"));
    }
    edits.push((
        pool.ints_count_at.0,
        pool.ints_count_at.1,
        enc_u30((pool.ints.len() + new_ints.len()) as u32),
    ));
    edits.push((
        pool.ints_end,
        pool.ints_end,
        new_ints.iter().flat_map(|v| enc_u30(*v as u32)).collect(),
    ));
    edits.push((
        pool.dbl_count_at.0,
        pool.dbl_count_at.1,
        enc_u30((pool.doubles.len() + new_dbls.len()) as u32),
    ));
    edits.push((
        pool.dbl_end,
        pool.dbl_end,
        new_dbls.iter().flat_map(|v| v.to_le_bytes()).collect(),
    ));
    let mut out = abc.to_vec();
    edits.sort_by(|a, b| b.0.cmp(&a.0));
    for (s, e, rep) in edits {
        out.splice(s..e, rep);
    }

    let idx_of = |name: &str, public_only: bool| -> Vec<u32> {
        pool.mns
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let m = m.as_ref()?;
                (m.qname && m.name == name && (!public_only || m.public)).then_some(i as u32)
            })
            .collect()
    };
    // ScreenSize cinit: findproperty SCREEN_WIDTH; pushshort 512; initproperty SCREEN_WIDTH
    let mut cinit = 0;
    for i in idx_of("SCREEN_WIDTH", true) {
        let e = enc_u30(i);
        let mut pat = vec![0x5e];
        pat.extend(&e);
        pat.push(0x25);
        pat.extend(enc_u30(ORIGINAL_WIDTH));
        pat.push(0x68);
        pat.extend(&e);
        if let Some(k) = find(&out, &pat, 0) {
            let at = k + 1 + e.len() + 1;
            out.splice(at..at + 2, enc_u30(w));
            cinit += 1;
        }
    }
    if cinit != 1 {
        return Err("ScreenSize initialiser not found".into());
    }
    // CharacterSelect: PLAYER_X_POS = GLOB_STG_RHT / 2  ->  512 / 2
    for px in idx_of("PLAYER_X_POS", false) {
        for gs in idx_of("GLOB_STG_RHT", false) {
            let (pe, ge) = (enc_u30(px), enc_u30(gs));
            let mut pat = vec![0x5e];
            pat.extend(&pe);
            pat.push(0x60);
            pat.extend(&ge);
            pat.extend([0x24, 0x02, 0xa3, 0x68]);
            pat.extend(&pe);
            let mut from = 0;
            while let Some(k) = find(&out, &pat, from) {
                let s = k + 1 + pe.len();
                let old_len = 1 + ge.len();
                let mut rep = vec![0x25];
                rep.extend(enc_u30(ORIGINAL_WIDTH));
                while rep.len() < old_len {
                    rep.push(0x02); // nop
                }
                if rep.len() == old_len {
                    out.splice(s..s + old_len, rep);
                }
                from = k + 1;
            }
        }
    }
    patch_projectile_limits(&mut out, &idx_of, w);
    Ok(out)
}

/// How many player projectiles may be on screen at once on a `w`-wide screen
/// when the game allows `n` on its 512 px screen: the same number per screen
/// width, so shots that fly to the edge leave as little downtime as before.
pub fn more_shots(n: u32, w: u32) -> u32 {
    let scaled = (n as f64 * w as f64 / ORIGINAL_WIDTH as f64).round() as u32;
    scaled.clamp(n, 127)
}

/// Static projectile limits are set in class initialisers
/// (`findproperty X; pushbyte n; initproperty X`), Simon's sub-weapon limit
/// (1, or 2/3 with the double/triple shot upgrade) in `Simon.canThrow`.
fn patch_projectile_limits(out: &mut [u8], idx_of: &dyn Fn(&str, bool) -> Vec<u32>, w: u32) {
    let statics: &[(&str, u8)] = &[
        ("MAX_BULLETS_ON_SCREEN", 3),                     // Sophia
        ("NUM_MEGA_BUSTER_SHOTS_ALLOWED", 3),             // Mega Man
        ("NUM_BASS_BUSTER_NO_MUSHROOM_SHOTS_ALLOWED", 3), // Bass
        ("NUM_BASS_BUSTER_SHOTS_ALLOWED", 4),             // Bass
        ("NUM_METAL_BLADES_ALLOWED", 3),                  // Mega Man / Bass
    ];
    for &(name, n) in statics {
        for i in idx_of(name, false) {
            let e = enc_u30(i);
            let mut pat = vec![0x5e];
            pat.extend(&e);
            pat.extend([0x24, n, 0x68]);
            pat.extend(&e);
            if let Some(k) = find(out, &pat, 0) {
                out[k + 1 + e.len() + 1] = more_shots(n as u32, w) as u8;
            }
        }
    }
    // Bill: NUM_FLARE_SHOTS_ALLOWED = 3 * 2
    for i in idx_of("NUM_FLARE_SHOTS_ALLOWED", false) {
        let e = enc_u30(i);
        let mut pat = vec![0x5e];
        pat.extend(&e);
        pat.extend([0x24, 3, 0x24, 2, 0xa2, 0x68]);
        pat.extend(&e);
        if let Some(k) = find(out, &pat, 0) {
            out[k + 1 + e.len() + 1] = more_shots(3, w) as u8;
        }
    }
    // Simon.canThrow: pushbyte 1; setlocal2; findpropstrict upgradeIsActive;
    // getlex SIMON_TRIPLE; ... pushbyte 3; setlocal2; ... pushbyte 2; setlocal2
    for ua in idx_of("upgradeIsActive", true) {
        for tr in idx_of("SIMON_TRIPLE", false) {
            let mut pat = vec![0x24, 1, 0xd6, 0x5d];
            pat.extend(enc_u30(ua));
            pat.push(0x60);
            pat.extend(enc_u30(tr));
            let Some(k) = find(out, &pat, 0) else { continue };
            // Locate all three before changing any (a new value may equal an old one).
            let end = (k + 64).min(out.len());
            let triple = find(&out[..end], &[0x24, 3, 0xd6], k + 3);
            let double = find(&out[..end], &[0x24, 2, 0xd6], k + 3);
            out[k + 1] = more_shots(1, w) as u8;
            if let Some(t) = triple {
                out[t + 1] = more_shots(3, w) as u8;
            }
            if let Some(t) = double {
                out[t + 1] = more_shots(2, w) as u8;
            }
        }
    }
}

fn read_rect(b: &[u8]) -> (Vec<i64>, usize) {
    let nbits = (b[0] >> 3) as usize;
    let total = 5 + 4 * nbits;
    let bit = |i: usize| (b[i / 8] >> (7 - i % 8)) & 1;
    let mut vals = Vec::new();
    let mut p = 5;
    for _ in 0..4 {
        let mut v: i64 = 0;
        for _ in 0..nbits {
            v = (v << 1) | bit(p) as i64;
            p += 1;
        }
        if nbits > 0 && v & (1 << (nbits - 1)) != 0 {
            v -= 1 << nbits;
        }
        vals.push(v);
    }
    (vals, total.div_ceil(8))
}

fn write_rect(vals: &[i64]) -> Vec<u8> {
    let nbits = vals
        .iter()
        .map(|v| 64 - v.unsigned_abs().leading_zeros() as usize + 1)
        .max()
        .unwrap_or(1);
    let mut bits: Vec<u8> = Vec::new();
    for i in (0..5).rev() {
        bits.push(((nbits >> i) & 1) as u8);
    }
    for v in vals {
        for i in (0..nbits).rev() {
            bits.push(((v >> i) & 1) as u8);
        }
    }
    while bits.len() % 8 != 0 {
        bits.push(0);
    }
    bits.chunks(8)
        .map(|c| c.iter().fold(0u8, |a, b| (a << 1) | b))
        .collect()
}

pub fn patch(data: &[u8], w: u32) -> Result<Vec<u8>, String> {
    if data.len() < 8 {
        return Err("not a SWF".into());
    }
    let version = data[3];
    let mut body = match &data[..3] {
        b"FWS" => data[8..].to_vec(),
        b"CWS" => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(&data[8..])
                .read_to_end(&mut out)
                .map_err(|e| e.to_string())?;
            out
        }
        _ => return Err("unsupported SWF compression".into()),
    };
    let (rect, rlen) = read_rect(&body);
    if rect[1] != (ORIGINAL_WIDTH * 20) as i64 {
        return Err(format!("unexpected stage width {}", rect[1] / 20));
    }
    let new_rect = write_rect(&[rect[0], (w * 20) as i64, rect[2], rect[3]]);
    body.splice(0..rlen, new_rect);
    let hdr = (5 + 4 * (body[0] >> 3) as usize).div_ceil(8) + 4;

    // Find the DoABC tag with the game code (the one containing Level).
    let mut p = hdr;
    let mut done = false;
    while p + 2 <= body.len() {
        let h = u16::from_le_bytes([body[p], body[p + 1]]);
        let hp = p;
        p += 2;
        let (t, mut l) = ((h >> 6) as u32, (h & 63) as usize);
        let long = l == 63;
        if long {
            l = u32::from_le_bytes(body[p..p + 4].try_into().unwrap()) as usize;
            p += 4;
        }
        if t == 82 && long && !done {
            let name_end = body[p + 4..p + l]
                .iter()
                .position(|&c| c == 0)
                .ok_or("bad DoABC")?
                + p
                + 4;
            let a0 = name_end + 1;
            let abc = &body[a0..p + l];
            if let Ok(new_abc) = patch_abc(abc, w) {
                let delta = new_abc.len() as isize - abc.len() as isize;
                body.splice(a0..p + l, new_abc);
                let nl = (l as isize + delta) as u32;
                body[hp + 2..hp + 6].copy_from_slice(&nl.to_le_bytes());
                l = nl as usize;
                done = true;
            }
        }
        p += l;
        if t == 0 {
            break;
        }
    }
    if !done {
        return Err("game code not found in movie".into());
    }
    let mut out = Vec::with_capacity(body.len() / 2);
    out.extend_from_slice(b"CWS");
    out.push(version);
    out.extend_from_slice(&((body.len() + 8) as u32).to_le_bytes());
    let mut enc = flate2::write::ZlibEncoder::new(out, flate2::Compression::fast());
    enc.write_all(&body).map_err(|e| e.to_string())?;
    enc.finish().map_err(|e| e.to_string())
}
