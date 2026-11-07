//! Super Mario Bros. Crossover: native replacement for its FlasCC-compiled libgme.
//!
//! SMBC renders all music and sound effects through Game Music Emu, compiled to
//! AVM2 bytecode with FlasCC. Interpreting that is far too slow, so calls to the
//! `libgme.gme_*` facade are routed to a native libgme instead. Emulator handles
//! given to ActionScript are opaque ids; audio is written into domain memory
//! exactly where the original `gme_play_float` wrapper would put it.

use super::function::{FunctionArgs, display_function};
use super::method::Method;
use super::{Activation, Error, Value};
use crate::string::{AvmString, WString};
use super::smbc_wide::WideFn;
use smbc_gme::{Emu, EmuType};
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GmeFn {
    TypeGbs,
    TypeKss,
    TypeNsf,
    TypeNsfe,
    TypeSap,
    TypeSpc,
    NewEmu,
    Delete,
    LoadData,
    StartTrack,
    Play,
    PlayFloat,
    TrackEnded,
    Tell,
    Seek,
    SetFade,
    SetTempo,
    SetStereoDepth,
    TrackCount,
    IgnoreSilence,
    /// `MenuBox.chooseItem`: handles the "exit to desktop" menu entry.
    MenuChooseItem,
}

/// Label of the main/pause menu entry that quits the program. The SWF patch
/// renames the (offline-useless) "links" entry to this.
pub const EXIT_ITEM: &str = "exit to desktop";

const ALL: [GmeFn; 21] = [
    GmeFn::TypeGbs, GmeFn::TypeKss, GmeFn::TypeNsf, GmeFn::TypeNsfe, GmeFn::TypeSap,
    GmeFn::TypeSpc, GmeFn::NewEmu, GmeFn::Delete, GmeFn::LoadData, GmeFn::StartTrack,
    GmeFn::Play, GmeFn::PlayFloat, GmeFn::TrackEnded, GmeFn::Tell, GmeFn::Seek,
    GmeFn::SetFade, GmeFn::SetTempo, GmeFn::SetStereoDepth, GmeFn::TrackCount,
    GmeFn::IgnoreSilence, GmeFn::MenuChooseItem,
];

fn classify(name: &str) -> Option<GmeFn> {
    if name == "com.smbc.messageBoxes::MenuBox/chooseItem()" {
        return Some(GmeFn::MenuChooseItem);
    }
    // Static methods of the SWIG facade class `libgme` (top-level package).
    let rest = name.strip_prefix("libgme$/")?;
    let rest = rest.strip_prefix("get ").unwrap_or(rest);
    let rest = rest.trim_end_matches("()");
    Some(match rest {
        "gme_gbs_type" => GmeFn::TypeGbs,
        "gme_kss_type" => GmeFn::TypeKss,
        "gme_nsf_type" => GmeFn::TypeNsf,
        "gme_nsfe_type" => GmeFn::TypeNsfe,
        "gme_sap_type" => GmeFn::TypeSap,
        "gme_spc_type" => GmeFn::TypeSpc,
        "gme_new_emu" => GmeFn::NewEmu,
        "gme_delete" => GmeFn::Delete,
        "gme_load_data" => GmeFn::LoadData,
        "gme_start_track" => GmeFn::StartTrack,
        "gme_play" => GmeFn::Play,
        "gme_play_float" => GmeFn::PlayFloat,
        "gme_track_ended" => GmeFn::TrackEnded,
        "gme_tell" => GmeFn::Tell,
        "gme_seek" => GmeFn::Seek,
        "gme_set_fade" => GmeFn::SetFade,
        "gme_set_tempo" => GmeFn::SetTempo,
        "gme_set_stereo_depth" => GmeFn::SetStereoDepth,
        "gme_track_count" => GmeFn::TrackCount,
        "gme_ignore_silence" => GmeFn::IgnoreSilence,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub enum Override {
    Gme(GmeFn),
    Wide(WideFn),
}

const WIDE: [WideFn; 11] = [
    WideFn::EndCredits,
    WideFn::EndText,
    WideFn::AddScorePop,
    WideFn::LayerScroll,
    WideFn::TopTextScroll,
    WideFn::CalcLocStg,
    WideFn::SaveSkinPreviews,
    WideFn::BlackScreen,
    WideFn::DictionaryCtor,
    WideFn::MapSkinMenuSwap,
    WideFn::TrimBmp,
];

pub fn lookup(method: Method<'_>) -> Option<Override> {
    let cell = &method.0.gme_override;
    match cell.get() {
        0 => {
            let mut name = WString::new();
            display_function(&mut name, method);
            let name_s = name.to_utf8_lossy();
            let found = if let Some(f) = classify(&name_s) {
                tracing::info!("SMBC: native override for {}", name);
                cell.set(f as u8 + 2);
                Some(Override::Gme(f))
            } else if let Some(w) = super::smbc_wide::classify(&name_s) {
                tracing::info!("SMBC: widescreen hook for {}", name);
                cell.set(200 + WIDE.iter().position(|x| *x == w).unwrap() as u8);
                Some(Override::Wide(w))
            } else {
                cell.set(1);
                None
            };
            found
        }
        1 => None,
        n if n >= 200 => Some(Override::Wide(WIDE[(n - 200) as usize])),
        n => Some(Override::Gme(ALL[(n - 2) as usize])),
    }
}

/// Opaque type ids handed to ActionScript (any distinct non-zero ints work).
const TYPE_BASE: i32 = 0x5e00;
/// Emulator ids handed to ActionScript.
const EMU_BASE: i32 = 0x5e10_0000;

thread_local! {
    static EMUS: RefCell<(i32, HashMap<i32, Emu>)> = RefCell::new((EMU_BASE, HashMap::new()));
}

fn with_emu<R>(id: i32, f: impl FnOnce(&mut Emu) -> R) -> Option<R> {
    EMUS.with(|e| e.borrow_mut().1.get_mut(&id).map(f))
}

fn result<'gc>(activation: &mut Activation<'_, 'gc>, r: Option<Result<(), String>>) -> Value<'gc> {
    match r {
        Some(Ok(())) => Value::Null,
        Some(Err(e)) => AvmString::new_utf8(activation.gc(), e).into(),
        None => AvmString::new_utf8(activation.gc(), "invalid emulator").into(),
    }
}

/// Returns `None` to let the original ActionScript method run.
pub fn call<'gc>(
    f: GmeFn,
    args: FunctionArgs<'_, 'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Option<Result<Value<'gc>, Error<'gc>>> {
    if f == GmeFn::MenuChooseItem {
        let item = args.to_slice().first().copied().unwrap_or(Value::Undefined);
        let is_exit = match item {
            Value::String(s) => s.to_utf8_lossy() == EXIT_ITEM,
            _ => false,
        };
        if is_exit {
            tracing::info!("SMBC: exit to desktop chosen");
            activation.context.external_interface.invoke_fs_command("quit", "");
            return Some(Ok(Value::Undefined));
        }
        return None;
    }
    Some(call_gme(f, args, activation))
}

fn call_gme<'gc>(
    f: GmeFn,
    args: FunctionArgs<'_, 'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Result<Value<'gc>, Error<'gc>> {
    let args = args.to_slice();
    let mut int = |i: usize, activation: &mut Activation<'_, 'gc>| -> Result<i32, Error<'gc>> {
        args.get(i).copied().unwrap_or(Value::Integer(0)).coerce_to_i32(activation)
    };
    let num = |i: usize, activation: &mut Activation<'_, 'gc>| -> Result<f64, Error<'gc>> {
        args.get(i).copied().unwrap_or(Value::Number(0.0)).coerce_to_number(activation)
    };

    Ok(match f {
        GmeFn::TypeGbs | GmeFn::TypeKss | GmeFn::TypeNsf | GmeFn::TypeNsfe | GmeFn::TypeSap
        | GmeFn::TypeSpc => Value::Integer(TYPE_BASE + f as i32),
        GmeFn::NewEmu => {
            let t = int(0, activation)? - TYPE_BASE;
            let rate = int(1, activation)?;
            let ty = match t {
                x if x == GmeFn::TypeGbs as i32 => EmuType::Gbs,
                x if x == GmeFn::TypeKss as i32 => EmuType::Kss,
                x if x == GmeFn::TypeNsf as i32 => EmuType::Nsf,
                x if x == GmeFn::TypeNsfe as i32 => EmuType::Nsfe,
                x if x == GmeFn::TypeSap as i32 => EmuType::Sap,
                x if x == GmeFn::TypeSpc as i32 => EmuType::Spc,
                _ => return Ok(Value::Integer(0)),
            };
            tracing::debug!("SMBC libgme: new {ty:?} emulator at {rate} Hz");
            match Emu::new(ty, rate) {
                Some(emu) => EMUS.with(|e| {
                    let mut e = e.borrow_mut();
                    e.0 += 1;
                    let id = e.0;
                    e.1.insert(id, emu);
                    Value::Integer(id)
                }),
                None => Value::Integer(0),
            }
        }
        GmeFn::Delete => {
            let id = int(0, activation)?;
            EMUS.with(|e| e.borrow_mut().1.remove(&id));
            Value::Undefined
        }
        GmeFn::LoadData => {
            let id = int(0, activation)?;
            let ptr = int(1, activation)?.max(0) as usize;
            let size = int(2, activation)?.max(0) as usize;
            let data = {
                let dm = activation.domain_memory();
                let dm = dm.storage();
                dm.bytes().get(ptr..ptr + size).map(|b| b.to_vec())
            };
            tracing::debug!("SMBC libgme: load_data {size} bytes");
            let r = match data {
                Some(data) => with_emu(id, |emu| emu.load_data(&data)),
                None => Some(Err("load_data: out of bounds".to_string())),
            };
            if let Some(Err(e)) = &r {
                tracing::warn!("SMBC libgme: load_data failed: {e}");
            }
            result(activation, r)
        }
        GmeFn::StartTrack => {
            let id = int(0, activation)?;
            let track = int(1, activation)?;
            let r = with_emu(id, |emu| emu.start_track(track));
            if let Some(Err(e)) = &r {
                tracing::warn!("SMBC libgme: start_track({track}) failed: {e}");
            }
            result(activation, r)
        }
        GmeFn::Play | GmeFn::PlayFloat => {
            let id = int(0, activation)?;
            let count = (int(1, activation)?.max(0) as usize) & !1;
            let buf_ptr = int(2, activation)?.max(0) as usize;
            let float_ptr = if f == GmeFn::PlayFloat {
                Some(int(3, activation)?.max(0) as usize)
            } else {
                None
            };
            let mut samples = vec![0i16; count];
            let r = with_emu(id, |emu| emu.play(&mut samples));
            if let Some(Ok(())) = r {
                let dm = activation.domain_memory();
                let mut dm = dm.storage_mut();
                let mem = dm.bytes_mut();
                if let Some(dst) = mem.get_mut(buf_ptr..buf_ptr + count * 2) {
                    for (d, s) in dst.chunks_exact_mut(2).zip(&samples) {
                        d.copy_from_slice(&s.to_le_bytes());
                    }
                }
                if let Some(fp) = float_ptr
                    && let Some(dst) = mem.get_mut(fp..fp + count * 4)
                {
                    for (d, s) in dst.chunks_exact_mut(4).zip(&samples) {
                        let v = (*s as f32) * (1.0 / 32768.0);
                        d.copy_from_slice(&v.to_le_bytes());
                    }
                }
            }
            if f == GmeFn::PlayFloat {
                // The original wrapper ignores the play error and returns null.
                Value::Null
            } else {
                result(activation, r)
            }
        }
        GmeFn::TrackEnded => {
            let id = int(0, activation)?;
            Value::Integer(with_emu(id, |emu| emu.track_ended() as i32).unwrap_or(1))
        }
        GmeFn::Tell => {
            let id = int(0, activation)?;
            Value::Integer(with_emu(id, |emu| emu.tell()).unwrap_or(0))
        }
        GmeFn::Seek => {
            let id = int(0, activation)?;
            let ms = int(1, activation)?;
            let r = with_emu(id, |emu| emu.seek(ms));
            result(activation, r)
        }
        GmeFn::SetFade => {
            let id = int(0, activation)?;
            let ms = int(1, activation)?;
            with_emu(id, |emu| emu.set_fade(ms));
            Value::Undefined
        }
        GmeFn::SetTempo => {
            let id = int(0, activation)?;
            let t = num(1, activation)?;
            with_emu(id, |emu| emu.set_tempo(t));
            Value::Undefined
        }
        GmeFn::SetStereoDepth => {
            let id = int(0, activation)?;
            let d = num(1, activation)?;
            with_emu(id, |emu| emu.set_stereo_depth(d));
            Value::Undefined
        }
        GmeFn::TrackCount => {
            let id = int(0, activation)?;
            Value::Integer(with_emu(id, |emu| emu.track_count()).unwrap_or(0))
        }
        GmeFn::IgnoreSilence | GmeFn::MenuChooseItem => Value::Undefined,
    })
}
