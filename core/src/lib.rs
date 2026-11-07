// This lint is helpful, but right now we have too many instances of it.
// TODO: Remove this once all instances are fixed.
#![allow(clippy::needless_pass_by_ref_mut)]
// This lint is good in theory, but in AVMs we often need to do `let x = args.get(0); let y = args.get(1);` etc.
// It'd make those much less readable and consistent.
#![allow(clippy::get_first)]

#[macro_use]
mod display_object;
pub use display_object::{StageAlign, StageDisplayState, StageScaleMode};

#[macro_use]
extern crate num_derive;

#[macro_use]
mod avm1;
mod avm2;
#[cfg(feature = "smbc_profile")]
pub use avm2::{profile_dump, profile_reset};
#[cfg(feature = "smbc_gme")]
pub use avm2::smbc_wide::set_save_paths as smbc_set_save_paths;
/// SMBC diagnostics: number of GPU -> CPU bitmap readbacks (each one stalls until the GPU catches up).
/// SMBC diagnostics: current length of the AVM2 orphan list.
pub static SMBC_ORPHANS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static SMBC_GPU_SYNCS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// SMBC diagnostics: number of BitmapData.draw offscreen renders.
pub static SMBC_OFFSCREEN_DRAWS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
mod avm_rng;
mod binary_data;
pub mod bitmap;
mod character;
pub mod context;
pub mod context_menu;
mod drawing;
mod ecma_conversions;
pub mod events;
pub mod focus_tracker;
pub mod font;
mod frame_lifecycle;
pub mod fte;
mod html;
mod input;
mod library;
pub mod limits;
pub mod loader;
mod local_connection;
mod net_connection;
mod orphan_manager;
pub mod pixel_bender;
mod player;
mod prelude;
pub mod socket;
mod streams;
pub mod string;
mod system_properties;
pub mod tag_utils;
mod tessellation_cache;
pub mod timer;
mod types;
mod vminterface;

pub mod backend;
pub mod compatibility_rules;
pub mod config;
#[cfg(feature = "egui")]
pub mod debug_ui;
pub mod external;
pub mod i18n;
pub mod stub;

pub use context_menu::ContextMenuItem;
pub use events::PlayerEvent;
pub use indexmap;
pub use loader::LoadBehavior;
pub use player::{Player, PlayerBuilder, PlayerMode, PlayerRuntime, StaticCallstack};
pub use ruffle_common::duration::FloatDuration;
pub use ruffle_render::backend::ViewportDimensions;
pub use swf;
pub use swf::Color;
pub use ttf_parser;

/// The newest Flash Player version known to Ruffle.
pub const NEWEST_PLAYER_VERSION: u8 = 51;

/// The default Flash Player version that Ruffle will emulate.
pub const DEFAULT_PLAYER_VERSION: u8 = 32;

/// Path to the raw playerglobal ABC, for use as an `asc.jar` import library.
pub const PLAYERGLOBAL_ABC_PATH: &str = env!("RUFFLE_PLAYERGLOBAL_ABC_PATH");
