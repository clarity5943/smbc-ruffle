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

use super::object::TObject;
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
        WideFn::DictionaryCtor | WideFn::MapSkinMenuSwap | WideFn::TrimBmp => Hook::Pass,
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

pub fn post<'gc>(f: WideFn, receiver: Value<'gc>, activation: &mut Activation<'_, 'gc>) {
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

