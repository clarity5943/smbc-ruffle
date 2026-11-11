//! Super Mario Bros. Crossover: widescreen support.
//!
//! `tools/widescreen.py` widens the movie's stage (and the game's screen-width
//! constants) to W pixels. These hooks fix up what the game itself can't cope
//! with on a stage wider than its original 512 px:
//!
//! * the HUD (`TopScreenText`, `HudAlwaysOnTop`) is drawn centred instead of at
//!   the left edge;
//! * maps narrower than the screen (one-screen rooms, the character-select
//!   backdrop, the x-2 intro areas, ...) are centred, the player can't leave
//!   them, and the part of the screen outside them is covered with black bars
//!   (drawn by `Stage::render_viewport`, see [`bars`]);
//! * skin previews are rendered with the off-screen `FakeLevel` at x = 0, as
//!   the original code assumes.
//!
//! Nothing here does anything for the original 512 px wide movie.

use super::object::{ArrayObject, TObject};
use super::array::ArrayStorage;
use super::{Activation, Error, Value};
use crate::display_object::{DisplayObject, TDisplayObject, TDisplayObjectContainer};
use crate::string::AvmString;
use std::cell::RefCell;

/// The game's own screen width.
const ORIGINAL_WIDTH: f64 = 512.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WideFn {
    /// `LevelGraphicLayerContainer.scroll()` (pre): runs right after the level's
    /// camera clamp, before the background/foreground/HUD read `level.x`.
    LayerScroll,
    /// `TopScreenText.scroll()` (replaced): keep the HUD centred.
    TopTextScroll,
    /// `Level.calcLocStgPos()` (post): keep the visible-area bounds inside narrow maps.
    CalcLocStg,
    /// `Character.saveSkinPreviews()` (pre): FakeLevel at x = 0.
    SaveSkinPreviews,
    /// `InformativeBlackScreen.initiate()` (pre): centre the "WORLD 1-1" /
    /// game over / time up screens.
    BlackScreen,
    /// `new Dictionary(weakKeys)` (pre): Ruffle ignored weak keys.
    DictionaryCtor,
    /// `LevelForeground.addScorePop(pop)`: score pops are placed at level
    /// coordinates inside `HudAlwaysOnTop`, which the layer-scroll hook shifts
    /// right to centre the HUD; take that shift back off the pop.
    AddScorePop,
    /// `ScreenManager.displayThankYouText()`, `addTxtTmrHandler()` and
    /// `startMoveCreditsTmrHandler()` (post): the castle-end texts ("Thank
    /// you", "...another castle", "Your quest is over", the credits) are
    /// centred on the stage; centre them on the map's last original-width
    /// screen instead, where the player and the Toad/Princess stand.
    EndText,
    /// `ScreenManager.startMoveCreditsTmrHandler()` (post): same centring for
    /// the credits; the raised "Your quest is over" goes back below the
    /// credits' black bars so it scrolls away like the credits do.
    EndCredits,
    /// `WaterHaze` constructor (post): the blue haze over water levels in the
    /// SMB All-Stars style skins is clipped by a 512 px wide mask; widen it to
    /// the stage.
    WaterHazeCtor,
    /// `CharacterSelectBox.getRandomCharInd()` (pre): the Random portrait was
    /// chosen; remember it so the character's version is randomized too.
    RandomCharPick,
    /// `CharacterSelect.chooseCharacter()` (pre): after a Random pick, give
    /// the player a random version (the game did this only for the special
    /// button, so jump always gave each character's current version).
    ChooseCharacter,
    /// `SmbcSaves.nativeRead(slot)` (replace): the slot file's text or null.
    SaveRead,
    /// `SmbcSaves.nativeWrite(slot, data)` (replace): write the slot file.
    SaveWrite,
    /// `SmbcSaves.nativeSetFileData(fileRef, data)` (replace): make the
    /// FileReference hold `data` as if a file had been picked.
    SaveSetFileData,
    /// `SmbcSaves.nativeClearFileRef()` (replace): `StatManager._fileRef = null`
    /// without the save handler's `relBtns()`.
    SaveClearFileRef,
    /// `StatManager.saveStartLevelStats()` (post): autosave.
    LevelStartStats,
    /// `GraphicsOptions.loadItems()` (post): drop "Random Map Skin" (it is
    /// chosen in the map skin menu now).
    GraphicsLoadItems,
    /// `MessageBox.destroy()` (post): a menu closed; save global settings.
    MsgBoxDestroy,
    /// `Cheats.unlockCheat()` / `unlockAllCheats()` (post): save global settings.
    CheatUnlock,
    /// `Cheats.loadData()` (post): a save was loaded; put the global settings
    /// back and merge its cheat unlocks into them.
    CheatsLoaded,
    /// `TitleLevel` constructor (post): apply global settings at startup.
    TitleCtor,
    /// `GameSettings.setTitleMapSkin()` (post): use the player's chosen map
    /// skin instead of the random title one.
    TitleMapSkin,
    /// `GameSettings.changeMapSkin()` (post): the player chose a map skin
    /// (ignored while a save's settings are being loaded).
    MapSkinChanged,
    /// `GameSettings.loadData()` (pre and post): a save's settings are loading.
    GsLoadData,
    /// `SmbcGlobal.nativeSetMapSkin(n)` (replace): `GameSettings._mapSkin = n`
    /// without redrawing (before the title screen is built).
    SetMapSkin,
    /// `OptionsMenu.loadItems()` (post): add "Controller" after "Set Buttons".
    OptionsLoadItems,
    /// `OptionsMenu.chooseItem()` (pre): "Controller" opens SmbcPadMenu.
    OptionsChoose,
    /// `SmbcPad.nativeSetKeys(keys)` (replace): the key each pad button sends.
    PadSetKeys,
    /// `SmbcPad.nativeCapture(on)` (replace): record pad presses instead.
    PadCapture,
    /// `SmbcPad.nativePoll()` (replace): the button pressed while capturing, or -1.
    PadPoll,
    /// `ScreenManager.createLevel()` (pre): in the single character modes,
    /// `SmbcStageSkin.intercept` shows the map skin menu before a new stage
    /// and runs createLevel itself when it closes.
    CreateLevel,
    /// `Level.loadNewLevel()` (pre): `SmbcSaves.leavingLevel(level, id)`
    /// notes whether the next level start is progress (for the autosave).
    LevelLeave,
    /// `CharacterSelectBox.pressedSelectCharacterButton()` and
    /// `GraphicsOptions.chooseItem()` (post): open `SmbcMapSkinMenu` (the map
    /// skin menu with a "Random" choice, added to the SWF by tools/patch_swf.py)
    /// instead of `MapSkinMenu`, also while random map skin is on (the game
    /// would only say it can't be changed).
    MapSkinMenuSwap,
    /// `CustomMovieClip.trimBmp()` (pre): Flash reports a mirrored object
    /// (negative determinant) with a negative scaleY, Ruffle with a positive one
    /// (and rotation 180), so trimmed sprite parts placed with a horizontal flip
    /// were shifted vertically (e.g. the right half of Samus's enemy explosion).
    TrimBmp,
}

pub fn classify(name: &str) -> Option<WideFn> {
    Some(match name {
        "com.smbc.level::LevelGraphicLayerContainer/scroll()" => WideFn::LayerScroll,
        "com.smbc.graphics::TopScreenText/scroll()" => WideFn::TopTextScroll,
        "com.smbc.characters::Character/saveSkinPreviews()" => WideFn::SaveSkinPreviews,
        "com.smbc.screens::InformativeBlackScreen/initiate()" => WideFn::BlackScreen,
        "flash.utils::Dictionary()" => WideFn::DictionaryCtor,
        "com.smbc.level::LevelForeground/addScorePop()" => WideFn::AddScorePop,
        "com.smbc.graphics::WaterHaze()" => WideFn::WaterHazeCtor,
        "com.smbc.level::CharacterSelect/chooseCharacter()" => WideFn::ChooseCharacter,
        "com.smbc.messageBoxes::SmbcSaves$/nativeRead()" => WideFn::SaveRead,
        "com.smbc.messageBoxes::MessageBox/destroy()" => WideFn::MsgBoxDestroy,
        "com.smbc.data::Cheats$/unlockCheat()" | "com.smbc.data::Cheats$/unlockAllCheats()" => WideFn::CheatUnlock,
        "com.smbc.data::Cheats$/loadData()" => WideFn::CheatsLoaded,
        "com.smbc.level::TitleLevel()" => WideFn::TitleCtor,
        "com.smbc.data::GameSettings$/setTitleMapSkin()" => WideFn::TitleMapSkin,
        "com.smbc.data::GameSettings$/changeMapSkin()" => WideFn::MapSkinChanged,
        "com.smbc.data::GameSettings$/loadData()" => WideFn::GsLoadData,
        "com.smbc.messageBoxes::SmbcGlobal$/nativeSetMapSkin()" => WideFn::SetMapSkin,
        "com.smbc.messageBoxes::SmbcPad$/nativeSetKeys()" => WideFn::PadSetKeys,
        "com.smbc.messageBoxes::SmbcPad$/nativeCapture()" => WideFn::PadCapture,
        "com.smbc.messageBoxes::SmbcPad$/nativePoll()" => WideFn::PadPoll,
        "com.smbc.messageBoxes::OptionsMenu/chooseItem()" => WideFn::OptionsChoose,
        "com.smbc.managers::ScreenManager/createLevel()" => WideFn::CreateLevel,
        n if n.starts_with("com.smbc.level::Level/") && n.ends_with("loadNewLevel()") => WideFn::LevelLeave,
        n if n.starts_with("com.smbc.messageBoxes::OptionsMenu/") && n.ends_with("loadItems()") => {
            WideFn::OptionsLoadItems
        }
        n if n.starts_with("com.smbc.messageBoxes::GraphicsOptions/") && n.ends_with("loadItems()") => {
            WideFn::GraphicsLoadItems
        }
        "com.smbc.messageBoxes::SmbcSaves$/nativeWrite()" => WideFn::SaveWrite,
        "com.smbc.messageBoxes::SmbcSaves$/nativeSetFileData()" => WideFn::SaveSetFileData,
        "com.smbc.messageBoxes::SmbcSaves$/nativeClearFileRef()" => WideFn::SaveClearFileRef,
        n if n.starts_with("com.smbc.managers::StatManager/") && n.ends_with("saveStartLevelStats()") => {
            WideFn::LevelStartStats
        }
        n if n.starts_with("com.smbc.messageBoxes::CharacterSelectBox/") && n.ends_with("getRandomCharInd()") => {
            WideFn::RandomCharPick
        }
        n if n.starts_with("com.smbc.managers::ScreenManager/")
            && (n.ends_with("displayThankYouText()")
                || n.ends_with("addTxtTmrHandler()")) =>
        {
            WideFn::EndText
        }
        n if n.starts_with("com.smbc.managers::ScreenManager/") && n.ends_with("startMoveCreditsTmrHandler()") => {
            WideFn::EndCredits
        }
        n if n.starts_with("com.smbc.messageBoxes::CharacterSelectBox/")
            && n.ends_with("pressedSelectCharacterButton()") =>
        {
            WideFn::MapSkinMenuSwap
        }
        n if n.starts_with("com.explodingRabbit.display::CustomMovieClip/") && n.ends_with("trimBmp()") => {
            WideFn::TrimBmp
        }
        n if n.starts_with("com.smbc.messageBoxes::GraphicsOptions/") && n.ends_with("chooseItem()") => {
            WideFn::MapSkinMenuSwap
        }
        n if n.starts_with("com.smbc.level::Level/") && n.ends_with("calcLocStgPos()") => {
            WideFn::CalcLocStg
        }
        _ => return None,
    })
}

pub enum Hook<'gc> {
    /// Run the original method.
    Pass,
    /// Run the original method, then call [`post`].
    Post,
    /// Don't run the original; return this instead.
    Replace(Result<Value<'gc>, Error<'gc>>),
}

/// The narrow map currently on screen: (level instance name, map width).
#[derive(Clone, Default)]
pub struct Bars {
    pub level_name: String,
    pub map_width: f64,
}

thread_local! {
    static BARS: RefCell<Option<Bars>> = const { RefCell::new(None) };
}

/// The narrow map that should be pillarboxed, if any.
pub fn bars() -> Option<Bars> {
    BARS.with(|b| b.borrow().clone())
}

fn stage_width(activation: &Activation<'_, '_>) -> f64 {
    activation.context.stage.movie_size().0 as f64
}

fn hud_offset(activation: &Activation<'_, '_>) -> f64 {
    ((stage_width(activation) - ORIGINAL_WIDTH) / 2.0).max(0.0).round()
}

fn class_name(obj: DisplayObject<'_>) -> String {
    obj.object2()
        .map(|o| o.instance_class().name().local_name().to_string())
        .unwrap_or_default()
}

fn num<'gc>(v: Value<'gc>, activation: &mut Activation<'_, 'gc>) -> f64 {
    v.coerce_to_number(activation).unwrap_or(f64::NAN)
}

fn get<'gc>(obj: Value<'gc>, name: &'static str, activation: &mut Activation<'_, 'gc>) -> Option<Value<'gc>> {
    obj.get_public_property(AvmString::new_utf8(activation.gc(), name), activation).ok()
}

fn set<'gc>(obj: Value<'gc>, name: &'static str, v: f64, activation: &mut Activation<'_, 'gc>) {
    let name = AvmString::new_utf8(activation.gc(), name);
    let _ = obj.set_public_property(name, Value::Number(v), activation);
}

fn is_level_class(n: &str) -> bool {
    n == "Level" || (n.ends_with("Level") && !n.starts_with("Level")) || n == "CharacterSelect"
}

pub fn call<'gc>(
    f: WideFn,
    receiver: Value<'gc>,
    args: super::function::FunctionArgs<'_, 'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Hook<'gc> {
    if f == WideFn::DictionaryCtor {
        let weak = args.to_slice().first().is_some_and(|v| v.coerce_to_boolean())
            && std::env::var_os("SMBC_NO_WEAK").is_none();
        if weak && let Some(d) = receiver.as_object().and_then(|o| o.as_dictionary_object()) {
            d.set_weak_keys(true);
        }
        return Hook::Pass;
    }
    if f == WideFn::MapSkinMenuSwap {
        return Hook::Post;
    }
    match f {
        WideFn::SaveRead | WideFn::SaveWrite | WideFn::SaveSetFileData | WideFn::SaveClearFileRef => {
            return Hook::Replace(save_native(f, args, activation));
        }
        WideFn::GsLoadData => {
            LOADING_SAVE_SETTINGS.with(|l| l.set(true));
            return Hook::Post;
        }
        WideFn::PadSetKeys | WideFn::PadCapture | WideFn::PadPoll => {
            return Hook::Replace(pad_native(f, args, activation));
        }
        WideFn::LevelLeave => {
            let id = args.to_slice().first().copied().unwrap_or(Value::Undefined);
            if let Some(c) = game_class(activation, "com.smbc.messageBoxes.SmbcSaves")
                && let Err(e) = c.call_public_property(
                    AvmString::new_utf8(activation.gc(), "leavingLevel"),
                    super::function::FunctionArgs::from_slice(&[receiver, id]),
                    activation,
                )
            {
                tracing::warn!("SMBC SmbcSaves.leavingLevel failed: {e:?}");
            }
            return Hook::Pass;
        }
        WideFn::CreateLevel => {
            let id = args.to_slice().first().copied().unwrap_or(Value::Undefined);
            let handled = game_class(activation, "com.smbc.messageBoxes.SmbcStageSkin").and_then(|c| {
                c.call_public_property(
                    AvmString::new_utf8(activation.gc(), "intercept"),
                    super::function::FunctionArgs::from_slice(&[receiver, id]),
                    activation,
                )
                .inspect_err(|e| tracing::warn!("SMBC SmbcStageSkin.intercept failed: {e:?}"))
                .ok()
            });
            if handled.is_some_and(|v| v.coerce_to_boolean()) {
                return Hook::Replace(Ok(Value::Undefined));
            }
            return Hook::Pass;
        }
        WideFn::OptionsChoose => {
            let name = args.to_slice().first().copied().and_then(|v| v.coerce_to_string(activation).ok());
            if name.is_some_and(|n| n.to_utf8_lossy() == "controller")
                && let Some(c) = game_class(activation, "com.smbc.messageBoxes.SmbcPadMenu")
                && let Err(e) = c.call_public_property(
                    AvmString::new_utf8(activation.gc(), "openFrom"),
                    super::function::FunctionArgs::from_slice(&[receiver]),
                    activation,
                )
            {
                tracing::warn!("SMBC SmbcPadMenu.openFrom failed: {e:?}");
            }
            return Hook::Pass;
        }
        WideFn::SetMapSkin => {
            let v = args.to_slice().first().copied().unwrap_or(Value::Undefined);
            if let Some(gs) = game_class(activation, "com.smbc.data.GameSettings") {
                set_any(gs, "_mapSkin", v, activation);
            }
            return Hook::Replace(Ok(Value::Undefined));
        }
        WideFn::LevelStartStats
        | WideFn::GraphicsLoadItems
        | WideFn::MsgBoxDestroy
        | WideFn::CheatUnlock
        | WideFn::CheatsLoaded
        | WideFn::TitleCtor
        | WideFn::TitleMapSkin
        | WideFn::MapSkinChanged
        | WideFn::OptionsLoadItems => return Hook::Post,
        _ => {}
    }
    if f == WideFn::RandomCharPick {
        // Only for a real character select (not the revive box or the
        // options' character-disable screen).
        let mode = get_any(receiver, "mode", activation).map(|v| v.coerce_to_string(activation).map(|s| s.to_utf8_lossy().into_owned()).unwrap_or_default());
        let cs = get_any(receiver, "CS_SCRN", activation).is_some_and(|v| v.as_object().is_some());
        RANDOM_PICK.with(|r| r.set(cs && mode.as_deref() == Some("characterSelect")));
        return Hook::Pass;
    }
    if f == WideFn::ChooseCharacter {
        if RANDOM_PICK.with(|r| r.replace(false)) && std::env::var_os("SMBC_NO_RANDOM_SKIN").is_none() {
            random_char_skin(receiver, activation);
        }
        return Hook::Pass;
    }
    if f == WideFn::TrimBmp {
        if std::env::var_os("SMBC_NO_TRIMFIX").is_none() {
            fix_trim_scale(args, activation);
        }
        return Hook::Pass;
    }
    let stage_w = stage_width(activation);
    if stage_w <= ORIGINAL_WIDTH {
        return Hook::Pass;
    }
    match f {
        WideFn::LayerScroll => {
            if std::env::var_os("SMBC_TEST_CHEATS").is_some() {
                test_cheats(activation);
            }
            if let Some(skin) = std::env::var("SMBC_TEST_SKIN").ok().and_then(|v| v.parse::<f64>().ok())
                && receiver
                    .as_object()
                    .and_then(|o| o.as_display_object())
                    .and_then(|d| d.parent())
                    .and_then(|p| p.as_container())
                    .is_some_and(|c| c.iter_render_list().any(|d| class_name(d) == "CharacterSelect"))
            {
                test_skin(skin, receiver, activation);
            }
            let Some(this) = receiver.as_object().and_then(|o| o.as_display_object()) else {
                return Hook::Pass;
            };
            let off = hud_offset(activation);
            if class_name(this) == "LevelForeground"
                && let Some(c) = this.as_container()
            {
                let huds: Vec<_> = c
                    .iter_render_list()
                    .filter(|d| class_name(*d) == "HudAlwaysOnTop")
                    .collect();
                for h in huds {
                    if let Some(o) = h.object2() {
                        set(o.into(), "x", off, activation);
                    }
                }
            }
            let Some(parent) = this.parent().and_then(|p| p.as_container()) else {
                return Hook::Pass;
            };
            let level = parent
                .iter_render_list()
                .find(|c| is_level_class(&class_name(*c)));
            let Some(level) = level else { return Hook::Pass };

            let Some(lv) = level.object2().map(Value::from) else { return Hook::Pass };
            let map_w = get(lv, "mapWidth", activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
            let screen_w = get(lv, "SCREEN_WIDTH", activation)
                .map(|v| num(v, activation))
                .unwrap_or(stage_w);
            if std::env::var_os("SMBC_WIDE_DEBUG").is_some() {
                eprintln!("[wide] {} map {map_w} screen {screen_w}", class_name(level));
            }
            if map_w.is_finite() && map_w > 0.0 && map_w < screen_w {
                let x = ((screen_w - map_w) / 2.0).round();
                set(lv, "x", x, activation);
                let name = level.name().map(|n| n.to_string()).unwrap_or_default();
                BARS.with(|b| *b.borrow_mut() = Some(Bars { level_name: name, map_width: map_w }));
            } else {
                BARS.with(|b| *b.borrow_mut() = None);
            }
            Hook::Pass
        }
        WideFn::TopTextScroll => {
            let off = hud_offset(activation);
            let lx = get(receiver, "level", activation)
                .filter(|l| !matches!(l, Value::Null | Value::Undefined))
                .and_then(|l| get(l, "x", activation))
                .map(|v| num(v, activation))
                .unwrap_or(0.0);
            set(receiver, "x", -lx + off, activation);
            Hook::Replace(Ok(Value::Undefined))
        }
        WideFn::CalcLocStg => Hook::Post,
        WideFn::AddScorePop => {
            if let Some(pop) = args.to_slice().first().copied().filter(|v| v.as_object().is_some()) {
                let off = hud_offset(activation);
                if let Some(x) = get(pop, "x", activation).map(|v| num(v, activation)) {
                    set(pop, "x", x - off, activation);
                }
            }
            Hook::Pass
        }
        WideFn::EndText | WideFn::EndCredits | WideFn::WaterHazeCtor => Hook::Post,
        WideFn::DictionaryCtor
        | WideFn::MapSkinMenuSwap
        | WideFn::TrimBmp
        | WideFn::RandomCharPick
        | WideFn::ChooseCharacter
        | WideFn::SaveRead
        | WideFn::SaveWrite
        | WideFn::SaveSetFileData
        | WideFn::SaveClearFileRef
        | WideFn::LevelStartStats
        | WideFn::GraphicsLoadItems
        | WideFn::MsgBoxDestroy
        | WideFn::CheatUnlock
        | WideFn::CheatsLoaded
        | WideFn::TitleCtor
        | WideFn::TitleMapSkin
        | WideFn::MapSkinChanged
        | WideFn::GsLoadData
        | WideFn::SetMapSkin
        | WideFn::OptionsLoadItems
        | WideFn::OptionsChoose
        | WideFn::PadSetKeys
        | WideFn::PadCapture
        | WideFn::PadPoll
        | WideFn::LevelLeave
        | WideFn::CreateLevel => Hook::Pass,
        WideFn::BlackScreen => {
            let off = hud_offset(activation);
            set(receiver, "x", off, activation);
            Hook::Post
        }
        WideFn::SaveSkinPreviews => {
            // Characters in the off-screen FakeLevel are snapshotted with code that
            // assumes their level sits at x = 0.
            let mut d = receiver.as_object().and_then(|o| o.as_display_object());
            while let Some(o) = d {
                if class_name(o) == "FakeLevel"
                    && let Some(obj) = o.object2()
                {
                    set(obj.into(), "x", 0.0, activation);
                }
                d = o.parent();
            }
            Hook::Pass
        }
    }
}

/// trimBmp(bmd, rect, [matrix, scaleX, scaleY, rotation])
fn fix_trim_scale<'gc>(args: super::function::FunctionArgs<'_, 'gc>, activation: &mut Activation<'_, 'gc>) {
    let Some(info) = args.to_slice().get(2).copied().filter(|v| v.as_object().is_some()) else {
        return;
    };
    let Some(matrix) = get(info, "0", activation).filter(|v| v.as_object().is_some()) else {
        return;
    };
    let mut m = [0.0f64; 4];
    for (i, k) in ["a", "b", "c", "d"].into_iter().enumerate() {
        m[i] = get(matrix, k, activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
    }
    let det = m[0] * m[3] - m[1] * m[2];
    let sy = get(info, "2", activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
    if det < 0.0 && sy > 0.0 {
        set(info, "2", -sy, activation);
    }
}

fn swap_map_skin_menu<'gc>(receiver: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
    let Some(next) = get(receiver, "nextMsgBxToCreate", activation).and_then(|v| v.as_object()) else {
        return;
    };
    let class = next.instance_class().name().local_name().to_string();
    let wanted = match class.as_str() {
        "MapSkinMenu" => true,
        // "You cannot change the map skin while random map skin is enabled."
        "PlainMessageBox" => {
            let settings = activation
                .domain()
                .get_defined_value_handling_vector(
                    activation,
                    AvmString::new_utf8(activation.gc(), "com.smbc.data.GameSettings"),
                )
                .ok();
            settings
                .and_then(|g| get(g, "randomMapSkin", activation))
                .is_some_and(|v| v.coerce_to_boolean())
        }
        _ => false,
    };
    if !wanted {
        return;
    }
    let menu_class = activation
        .domain()
        .get_defined_value_handling_vector(
            activation,
            AvmString::new_utf8(activation.gc(), "com.smbc.messageBoxes.SmbcMapSkinMenu"),
        )
        .ok()
        .and_then(|v| v.as_object())
        .and_then(|o| o.as_class_object());
    let Some(menu_class) = menu_class else {
        return; // older SWF without the addition
    };
    match menu_class.construct(activation, &[]) {
        Ok(menu) => {
            let name = AvmString::new_utf8(activation.gc(), "nextMsgBxToCreate");
            let _ = receiver.set_public_property(name, menu, activation);
        }
        Err(e) => tracing::warn!("SMBC: SmbcMapSkinMenu failed: {e:?}"),
    }
}

/// The black screen between levels is only 512 px wide; the castle-end texts
/// moved over the last room would show beside it, so hide them (the game
/// removes them when the next level starts).
fn hide_end_text<'gc>(receiver: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
    let Some(game) = receiver
        .as_object()
        .and_then(|o| o.as_display_object())
        .and_then(|d| d.parent())
        .and_then(|p| p.as_container())
    else {
        return;
    };
    let moved = MOVED_END_TEXT.with(|m| m.borrow().1.clone());
    let texts: Vec<_> = game
        .iter_render_list()
        .filter(|d| class_name(*d) == "TextFieldContainer")
        .filter_map(|d| d.object2())
        .map(Value::from)
        .filter(|v| v.as_object().is_some_and(|o| moved.contains(&(o.as_ptr() as usize))))
        .collect();
    for t in texts {
        let _ = t.set_public_property(AvmString::new_utf8(activation.gc(), "visible"), Value::Bool(false), activation);
    }
}

/// Bench-only (`SMBC_TEST_SKIN=n`): map skin n, set at the character select.
fn test_skin<'gc>(skin: f64, receiver: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
    let domain = receiver
        .as_object()
        .and_then(|o| o.instance_class().translation_unit())
        .map(|t| t.domain())
        .unwrap_or_else(|| activation.domain());
    let Ok(gs) = domain.get_defined_value_handling_vector(activation, AvmString::new_utf8(activation.gc(), "com.smbc.data.GameSettings")) else {
        return;
    };
    if get(gs, "mapSkin", activation).map(|v| num(v, activation)) == Some(skin) {
        return;
    }
    let _ = gs.call_public_property(
        AvmString::new_utf8(activation.gc(), "changeMapSkin"),
        super::function::FunctionArgs::from_slice(&[Value::Number(skin)]),
        activation,
    );
}

/// Bench-only (`SMBC_TEST_CHEATS`): invincible, bouncy pits, infinite time.
fn test_cheats<'gc>(activation: &mut Activation<'_, 'gc>) {
    let Ok(c) = activation
        .domain()
        .get_defined_value_handling_vector(activation, AvmString::new_utf8(activation.gc(), "com.smbc.data.Cheats"))
    else {
        return;
    };
    for k in ["invincible", "bouncyPits", "infiniteTime"] {
        let name = AvmString::new_utf8(activation.gc(), k);
        let _ = c.set_public_property(name, Value::Bool(true), activation);
    }
}

thread_local! {
    /// Inside `GameSettings.loadData` (a save's settings are being loaded).
    static LOADING_SAVE_SETTINGS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

thread_local! {
    /// The Random portrait was just used to pick a character.
    static RANDOM_PICK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A property by local name in whatever namespace it was declared (private
/// fields included).
fn get_any<'gc>(obj: Value<'gc>, name: &str, activation: &mut Activation<'_, 'gc>) -> Option<Value<'gc>> {
    let o = obj.as_object()?;
    let ns = o
        .vtable()
        .resolved_traits()
        .iter()
        .find(|(n, _, _)| n.to_utf8_lossy() == name)
        .map(|(_, ns, _)| ns)?;
    let mn = super::Multiname::new(ns, AvmString::new_utf8(activation.gc(), name));
    obj.get_property(&mn, activation).ok()
}

/// `GraphicsManager.INSTANCE.changeCharacterSkin(player, CHAR_SKIN_NUM_RANDOM)`.
fn random_char_skin<'gc>(receiver: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
    let Some(player) = get(receiver, "player", activation).filter(|v| v.as_object().is_some()) else { return };
    let domain = receiver
        .as_object()
        .and_then(|o| o.instance_class().translation_unit())
        .map(|t| t.domain())
        .unwrap_or_else(|| activation.domain());
    let Ok(gm_class) = domain.get_defined_value_handling_vector(activation, AvmString::new_utf8(activation.gc(), "com.smbc.managers.GraphicsManager")) else {
        return;
    };
    let Some(gm) = get(gm_class, "INSTANCE", activation).filter(|v| v.as_object().is_some()) else { return };
    let random = get(gm_class, "CHAR_SKIN_NUM_RANDOM", activation).unwrap_or(Value::Number(-2.0));
    let _ = gm.call_public_property(
        AvmString::new_utf8(activation.gc(), "changeCharacterSkin"),
        super::function::FunctionArgs::from_slice(&[player, random]),
        activation,
    );
    if std::env::var_os("SMBC_WIDE_DEBUG").is_some() {
        let c = get(player, "charNum", activation).map(|v| num(v, activation));
        let skin = get(player, "currentBmdSkin", activation).and_then(|v| v.as_object().map(|_| v)).and_then(|v| get(v, "skinNum", activation)).map(|v| num(v, activation));
        eprintln!("[random] char {c:?} skin {skin:?}");
    }
}

thread_local! {
    /// (level, castle-end texts already moved for it)
    static MOVED_END_TEXT: std::cell::RefCell<(usize, Vec<usize>)> = const { std::cell::RefCell::new((0, Vec::new())) };
}

/// Move stage-centred `TextFieldContainer`s that sit beside the level to the
/// centre of the map's last 512 px. The game puts them just below the level,
/// where a wall in the wider view can hide them, so the castle texts are also
/// raised to just above the level (still below the HUD).
fn center_end_text<'gc>(credits: bool, receiver: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
    let stage_w = stage_width(activation);
    if stage_w <= ORIGINAL_WIDTH {
        return;
    }
    // Called from a timer event the activation's domain is the system one, so
    // look the class up in the domain of the receiver's (game) class.
    let domain = receiver
        .as_object()
        .and_then(|o| o.instance_class().translation_unit())
        .map(|t| t.domain())
        .unwrap_or_else(|| activation.domain());
    let level = domain
        .get_defined_value_handling_vector(activation, AvmString::new_utf8(activation.gc(), "com.smbc.level.Level"))
        .ok()
        .and_then(|c| get(c, "levelInstance", activation))
        .filter(|v| v.as_object().is_some());
    let Some(level) = level else { return };
    let Some(level_do) = level.as_object().and_then(|o| o.as_display_object()) else { return };
    let Some(game) = level_do.parent().and_then(|p| p.as_container()) else { return };
    let map_w = get(level, "mapWidth", activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
    let lx = get(level, "x", activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
    if !(map_w.is_finite() && lx.is_finite() && map_w > 0.0) {
        return;
    }
    let center = (lx + map_w - ORIGINAL_WIDTH / 2.0).round();
    let level_ptr = level.as_object().map(|o| o.as_ptr() as usize).unwrap_or(0);
    let texts: Vec<_> = game
        .iter_render_list()
        .filter(|d| class_name(*d) == "TextFieldContainer")
        .filter_map(|d| d.object2())
        .collect();
    let shift = center - stage_w / 2.0;
    let mut fresh_texts = Vec::new();
    let mut old_texts = Vec::new();
    for t in texts {
        let ptr = Value::from(t).as_object().map(|o| o.as_ptr() as usize).unwrap_or(0);
        let fresh = MOVED_END_TEXT.with(|m| {
            let mut m = m.borrow_mut();
            if m.0 != level_ptr {
                *m = (level_ptr, Vec::new());
            }
            if m.1.contains(&ptr) {
                false
            } else {
                m.1.push(ptr);
                true
            }
        });
        let t: Value<'gc> = t.into();
        if fresh {
            let x = get(t, "x", activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
            if x.is_finite() {
                set(t, "x", x + shift, activation);
            }
            fresh_texts.push(t);
        } else {
            old_texts.push(t);
        }
    }
    let Some(game_v) = level_do.parent().and_then(|p| p.object2()).map(Value::from) else { return };
    let index_of = |v: Value<'gc>, activation: &mut Activation<'_, 'gc>| {
        game_v
            .call_public_property(
                AvmString::new_utf8(activation.gc(), "getChildIndex"),
                super::function::FunctionArgs::from_slice(&[v]),
                activation,
            )
            .ok()
            .map(|i| num(i, activation))
    };
    let set_index = |v: Value<'gc>, i: f64, activation: &mut Activation<'_, 'gc>| {
        let _ = game_v.call_public_property(
            AvmString::new_utf8(activation.gc(), "setChildIndex"),
            super::function::FunctionArgs::from_slice(&[v, Value::Number(i)]),
            activation,
        );
    };
    if !credits {
        // Just above the level (the HUD layer is added after the level, so it stays on top).
        for t in fresh_texts {
            if let Some(li) = index_of(level, activation) {
                set_index(t, li, activation);
            }
        }
    } else {
        // Back down to just below the credits text.
        let lowest = fresh_texts
            .iter()
            .filter_map(|t| index_of(*t, activation))
            .fold(f64::INFINITY, f64::min);
        let li = index_of(level, activation).unwrap_or(f64::NAN);
        if lowest.is_finite() {
            for t in old_texts {
                if index_of(t, activation).is_some_and(|i| i > li) {
                    set_index(t, lowest, activation);
                }
            }
        }
    }
}

pub fn post<'gc>(f: WideFn, receiver: Value<'gc>, ret: Option<Value<'gc>>, activation: &mut Activation<'_, 'gc>) {
    if f == WideFn::GsLoadData {
        LOADING_SAVE_SETTINGS.with(|l| l.set(false));
        return;
    }
    let global = match f {
        WideFn::MsgBoxDestroy | WideFn::CheatUnlock => Some("save"),
        WideFn::CheatsLoaded => Some("afterSaveLoaded"),
        WideFn::TitleCtor => Some("startup"),
        WideFn::TitleMapSkin => Some("titleMapSkin"),
        WideFn::MapSkinChanged if !LOADING_SAVE_SETTINGS.with(|l| l.get()) => Some("mapSkinChosen"),
        WideFn::MapSkinChanged => return,
        _ => None,
    };
    if let Some(method) = global {
        if let Some(c) = game_class(activation, "com.smbc.messageBoxes.SmbcGlobal")
            && let Err(e) = c.call_public_property(
                AvmString::new_utf8(activation.gc(), method),
                super::function::FunctionArgs::empty(),
                activation,
            )
        {
            tracing::warn!("SMBC SmbcGlobal.{method} failed: {e:?}");
        }
        return;
    }
    if f == WideFn::OptionsLoadItems {
        if let Some(items) = ret.filter(|v| v.as_object().is_some()) {
            let entry = ArrayObject::from_storage(
                activation.context,
                ArrayStorage::from_args(&[AvmString::new_utf8(activation.gc(), "controller").into()]),
            );
            let _ = items.call_public_property(
                AvmString::new_utf8(activation.gc(), "splice"),
                super::function::FunctionArgs::from_slice(&[Value::Number(1.0), Value::Number(0.0), entry.into()]),
                activation,
            );
        }
        return;
    }
    if f == WideFn::GraphicsLoadItems {
        remove_menu_item(ret, "Random Map Skin", activation);
        return;
    }
    if f == WideFn::LevelStartStats {
        if let Some(c) = game_class(activation, "com.smbc.messageBoxes.SmbcSaves")
            && let Err(e) = c.call_public_property(
                AvmString::new_utf8(activation.gc(), "autosave"),
                super::function::FunctionArgs::empty(),
                activation,
            )
        {
            tracing::warn!("SMBC autosave failed: {e:?}");
        }
        // Bench only: finish the first few stages at once (autosave test).
        thread_local!(static BEAT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) });
        if std::env::var_os("SMBC_TEST_BEAT").is_some()
            && let Some(c) = game_class(activation, "com.smbc.level.Level")
            && let Ok(lev) = c.get_public_property(AvmString::new_utf8(activation.gc(), "levelInstance"), activation)
            && lev.as_object().is_some_and(|o| o.instance_class().name().local_name().to_utf8_lossy() == "Level")
            && BEAT.with(|b| { b.set(b.get() + 1); b.get() <= 2 })
        {
            let r = lev.set_public_property(AvmString::new_utf8(activation.gc(), "beatLevel"), Value::Bool(true), activation);
            tracing::warn!("SMBC test beat: {r:?}");
        }
        return;
    }
    if f == WideFn::BlackScreen {
        hide_end_text(receiver, activation);
        return;
    }
    if f == WideFn::WaterHazeCtor {
        if let Some(mask) = get(receiver, "mask", activation).filter(|v| v.as_object().is_some()) {
            let w = stage_width(activation);
            set(mask, "width", w, activation);
        }
        return;
    }
    if f == WideFn::EndText || f == WideFn::EndCredits {
        center_end_text(f == WideFn::EndCredits, receiver, activation);
        return;
    }
    if f == WideFn::MapSkinMenuSwap {
        swap_map_skin_menu(receiver, activation);
        return;
    }
    if f != WideFn::CalcLocStg {
        return;
    }

    let map_w = get(receiver, "mapWidth", activation).map(|v| num(v, activation)).unwrap_or(f64::NAN);
    let screen_w = get(receiver, "SCREEN_WIDTH", activation)
        .map(|v| num(v, activation))
        .unwrap_or(f64::NAN);
    if !(map_w.is_finite() && screen_w.is_finite() && map_w > 0.0 && map_w < screen_w) {
        return;
    }
    let lft = get(receiver, "locStgLft", activation).map(|v| num(v, activation)).unwrap_or(0.0);
    let rht = get(receiver, "locStgRht", activation).map(|v| num(v, activation)).unwrap_or(map_w);
    if lft < 0.0 {
        set(receiver, "locStgLft", 0.0, activation);
    }
    if rht > map_w {
        set(receiver, "locStgRht", map_w, activation);
    }
}


// ---------------------------------------------------------------------------
// In-game save slots. The game's own save code is kept: `FileReference.save`
// and `browse` (file_reference.rs) call `SmbcSaves.requestSave/requestLoad`
// (AS3, added to the SWF) instead of opening system dialogs, and its slot
// menu reads/writes plain text files (the game's save format) here.

/// Slot 0 is the autosave; 1..=5 are the player's slots.
const SAVE_SLOT_MAX: i32 = 5;

static SAVE_PATHS: std::sync::OnceLock<(std::path::PathBuf, Option<std::path::PathBuf>)> =
    std::sync::OnceLock::new();

/// Set by the desktop bundle: the slot directory, and where to find old
/// `smbc_save_data*.txt` files to bring into slots 1-5 the first time (a
/// file or a folder).
pub fn set_save_paths(dir: std::path::PathBuf, import: Option<std::path::PathBuf>) {
    let _ = SAVE_PATHS.set((dir, import));
}

fn slot_dir() -> std::path::PathBuf {
    SAVE_PATHS
        .get()
        .map(|p| p.0.clone())
        .or_else(|| std::env::var_os("SMBC_SAVE_SLOTS_DIR").map(Into::into))
        .unwrap_or_else(|| std::env::temp_dir().join("smbc_save_slots"))
}

/// `nativeRead/nativeWrite` slot number of the global settings file.
const GLOBAL_SETTINGS_SLOT: i32 = 100;

fn slot_path(slot: i32) -> std::path::PathBuf {
    slot_dir().join(match slot {
        0 => "autosave.txt".to_string(),
        GLOBAL_SETTINGS_SLOT => "settings.txt".to_string(),
        n => format!("slot{n}.txt"),
    })
}

fn import_old_save() {
    static DONE: std::sync::Once = std::sync::Once::new();
    DONE.call_once(|| {
        let dir = slot_dir();
        let marker = dir.join(".imported");
        if marker.exists() {
            return;
        }
        let _ = std::fs::create_dir_all(&dir);
        // A file, or a folder holding smbc_save_data*.txt (the game's own
        // save files, e.g. next to the exe).
        let import = SAVE_PATHS
            .get()
            .and_then(|p| p.1.clone())
            .or_else(|| std::env::var_os("SMBC_IMPORT_SAVE").map(Into::into));
        let mut files: Vec<std::path::PathBuf> = match import {
            Some(p) if p.is_file() => vec![p],
            Some(p) if p.is_dir() => std::fs::read_dir(&p)
                .map(|rd| {
                    rd.filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|f| {
                            f.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                                let n = n.to_ascii_lowercase();
                                n.starts_with("smbc_save_data") && n.ends_with(".txt")
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        files.sort();
        let mut slot = 1;
        for src in files {
            while slot <= SAVE_SLOT_MAX && slot_path(slot).exists() {
                slot += 1;
            }
            if slot > SAVE_SLOT_MAX {
                break;
            }
            let Ok(data) = std::fs::read(&src) else { continue };
            if !String::from_utf8_lossy(&data).contains("<smbc_save_data>") {
                continue;
            }
            // Contents only (a copy would keep a read-only attribute), with the
            // file's date as the save time.
            let mut out = Vec::new();
            if let Ok(t) = std::fs::metadata(&src).and_then(|m| m.modified())
                && let Ok(d) = t.duration_since(std::time::UNIX_EPOCH)
            {
                out.extend_from_slice(format!("#smbc-slot {}\n", d.as_millis()).as_bytes());
            }
            out.extend_from_slice(&data);
            match std::fs::write(slot_path(slot), out) {
                Ok(()) => tracing::info!("SMBC: imported {} into save slot {slot}", src.display()),
                Err(e) => tracing::warn!("SMBC: couldn't import {}: {e}", src.display()),
            }
        }
        let _ = std::fs::write(marker, b"");
    });
}

fn slot_read(slot: i32) -> Option<String> {
    if !(0..=SAVE_SLOT_MAX).contains(&slot) && slot != GLOBAL_SETTINGS_SLOT {
        return None;
    }
    import_old_save();
    let bytes = std::fs::read(slot_path(slot)).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn slot_write(slot: i32, data: &str) -> bool {
    if !(0..=SAVE_SLOT_MAX).contains(&slot) && slot != GLOBAL_SETTINGS_SLOT {
        return false;
    }
    import_old_save();
    let path = slot_path(slot);
    let tmp = path.with_extension("tmp");
    let ok = std::fs::create_dir_all(slot_dir()).is_ok()
        && std::fs::write(&tmp, data.as_bytes()).is_ok()
        && std::fs::rename(&tmp, &path).is_ok();
    if !ok {
        tracing::warn!("SMBC: couldn't write {}", path.display());
    }
    ok
}

/// A FileReference "selection" held in memory.
struct MemFile(Vec<u8>);

impl crate::backend::ui::FileDialogSelection for MemFile {
    fn creation_time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None
    }
    fn modification_time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        None
    }
    fn file_name(&self) -> String {
        "smbc_save_data.txt".into()
    }
    fn size(&self) -> Option<u64> {
        Some(self.0.len() as u64)
    }
    fn file_type(&self) -> Option<String> {
        Some(".txt".into())
    }
    fn contents(&self) -> &[u8] {
        &self.0
    }
    fn write_and_refresh(&mut self, data: &[u8]) {
        self.0 = data.to_vec();
    }
}

fn save_native<'gc>(
    f: WideFn,
    args: super::function::FunctionArgs<'_, 'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Result<Value<'gc>, Error<'gc>> {
    let a = args.to_slice();
    let arg = |i: usize| a.get(i).copied().unwrap_or(Value::Undefined);
    match f {
        WideFn::SaveRead => {
            let slot = arg(0).coerce_to_i32(activation)?;
            Ok(slot_read(slot).map_or(Value::Null, |s| AvmString::new_utf8(activation.gc(), s).into()))
        }
        WideFn::SaveWrite => {
            let slot = arg(0).coerce_to_i32(activation)?;
            let data = arg(1).coerce_to_string(activation)?.to_utf8_lossy().into_owned();
            Ok(slot_write(slot, &data).into())
        }
        WideFn::SaveSetFileData => {
            let data = arg(1).coerce_to_string(activation)?.to_utf8_lossy().into_owned();
            if let Some(fr) = arg(0).as_object().and_then(|o| o.as_file_reference()) {
                fr.init_from_file_selection(Box::new(MemFile(data.into_bytes())));
                fr.set_loaded(false);
            }
            Ok(Value::Undefined)
        }
        WideFn::SaveClearFileRef => {
            if let Some(sm) = game_class(activation, "com.smbc.managers.StatManager")
                .and_then(|c| get(c, "STAT_MNGR", activation))
                .filter(|v| v.as_object().is_some())
            {
                set_any(sm, "_fileRef", Value::Null, activation);
            }
            Ok(Value::Undefined)
        }
        _ => Ok(Value::Undefined),
    }
}

/// Remove the `[name, ...]` entry from a menu's item array.
fn remove_menu_item<'gc>(items: Option<Value<'gc>>, name: &str, activation: &mut Activation<'_, 'gc>) {
    let Some(items) = items.filter(|v| v.as_object().is_some()) else { return };
    let len = get(items, "length", activation).map(|v| num(v, activation)).unwrap_or(0.0) as usize;
    for i in 0..len {
        let key = AvmString::new_utf8(activation.gc(), i.to_string());
        let Ok(item) = items.get_public_property(key, activation) else { continue };
        let first = item
            .as_object()
            .and_then(|_| item.get_public_property(AvmString::new_utf8(activation.gc(), "0"), activation).ok())
            .and_then(|v| v.coerce_to_string(activation).ok())
            .map(|s| s.to_utf8_lossy().into_owned());
        if first.as_deref() == Some(name) {
            let _ = items.call_public_property(
                AvmString::new_utf8(activation.gc(), "splice"),
                super::function::FunctionArgs::from_slice(&[Value::Number(i as f64), Value::Number(1.0)]),
                activation,
            );
            return;
        }
    }
}

/// A class defined by the game SWF (looked up in the root movie's domain, so
/// it works from natives and event handlers too).
fn game_class<'gc>(activation: &mut Activation<'_, 'gc>, name: &str) -> Option<Value<'gc>> {
    let movie = *activation.context.root_swf;
    let domain = activation.context.library.library_for_movie(movie)?.try_avm2_domain()?;
    domain
        .get_defined_value_handling_vector(activation, AvmString::new_utf8(activation.gc(), name))
        .ok()
}

/// Set a property in whatever namespace it was declared (private included).
fn set_any<'gc>(obj: Value<'gc>, name: &str, v: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
    let Some(o) = obj.as_object() else { return };
    let Some(ns) = o
        .vtable()
        .resolved_traits()
        .iter()
        .find(|(n, _, _)| n.to_utf8_lossy() == name)
        .map(|(_, ns, _)| ns)
    else {
        return;
    };
    let mn = super::Multiname::new(ns, AvmString::new_utf8(activation.gc(), name));
    let _ = obj.set_property(&mn, v, activation);
}

/// Called by `FileReference.browse` (`data == None`) and `save`: hand the
/// request to the game's `SmbcSaves` instead of a system dialog. Returns
/// false when the SWF has no `SmbcSaves` (any other movie).
pub fn save_redirect<'gc>(activation: &mut Activation<'_, 'gc>, file_ref: Value<'gc>, data: Option<&[u8]>) -> bool {
    let Some(c) = game_class(activation, "com.smbc.messageBoxes.SmbcSaves") else {
        return false;
    };
    let (method, args) = match data {
        None => ("requestLoad", vec![file_ref]),
        Some(d) => (
            "requestSave",
            vec![file_ref, AvmString::new_utf8(activation.gc(), String::from_utf8_lossy(d)).into()],
        ),
    };
    if let Err(e) = c.call_public_property(
        AvmString::new_utf8(activation.gc(), method),
        super::function::FunctionArgs::from_slice(&args),
        activation,
    ) {
        tracing::warn!("SMBC SmbcSaves.{method} failed: {e:?}");
    }
    true
}

/// The game's Controller menu (AS3 `SmbcPad`): set the key each pad button
/// sends, and capture a pad press for assigning.
fn pad_native<'gc>(
    f: WideFn,
    args: super::function::FunctionArgs<'_, 'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Result<Value<'gc>, Error<'gc>> {
    use crate::input::{SMBC_PAD_CAPTURE, SMBC_PAD_KEYS, SMBC_PAD_LAST};
    use std::sync::atomic::Ordering;
    let a = args.to_slice();
    match f {
        WideFn::PadSetKeys => {
            let mut keys = [0u32; 14];
            if let Some(arr) = a.first().copied().filter(|v| v.as_object().is_some()) {
                for (i, k) in keys.iter_mut().enumerate() {
                    let v = arr.get_public_property(AvmString::new_utf8(activation.gc(), i.to_string()), activation)?;
                    *k = v.coerce_to_i32(activation)?.max(0) as u32;
                }
            }
            if let Ok(mut g) = SMBC_PAD_KEYS.lock() {
                *g = Some(keys);
            }
            Ok(Value::Undefined)
        }
        WideFn::PadCapture => {
            let on = a.first().copied().unwrap_or(Value::Undefined).coerce_to_boolean();
            SMBC_PAD_LAST.store(-1, Ordering::Relaxed);
            SMBC_PAD_CAPTURE.store(on, Ordering::Relaxed);
            Ok(Value::Undefined)
        }
        WideFn::PadPoll => Ok(Value::Integer(SMBC_PAD_LAST.swap(-1, Ordering::Relaxed))),
        _ => Ok(Value::Undefined),
    }
}
