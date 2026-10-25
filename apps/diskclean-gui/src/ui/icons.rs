//! Phosphor icon codepoints used by the shell.
//!
//! The font is embedded in `assets/icons/Phosphor.ttf` (MIT/Apache-2.0). The
//! crate `egui-phosphor` currently targets egui 0.36, so we keep only the small
//! set of codepoints we need and register the font ourselves.

#![allow(dead_code)]

pub(crate) const APP: &str = "\u{E2A0}"; // hard drives
pub(crate) const VOLUME: &str = "\u{E29E}"; // hard drive
pub(crate) const FOLDER: &str = "\u{E24A}";
pub(crate) const FOLDER_OPEN: &str = "\u{E256}";
pub(crate) const FILE: &str = "\u{E230}";
pub(crate) const SEARCH: &str = "\u{E30C}";
pub(crate) const FILTER: &str = "\u{E266}";
pub(crate) const SETTINGS: &str = "\u{E270}";
pub(crate) const HELP: &str = "\u{E3E8}";
pub(crate) const ARROW_UP: &str = "\u{E08E}";
pub(crate) const CARET_RIGHT: &str = "\u{E13A}";
pub(crate) const CARET_DOWN: &str = "\u{E136}";
pub(crate) const CARET_LEFT: &str = "\u{E138}";
pub(crate) const CARET_UP: &str = "\u{E13C}";
pub(crate) const CLOSE: &str = "\u{E4F6}";
pub(crate) const RETRY: &str = "\u{E036}"; // arrow clockwise
pub(crate) const BROOM: &str = "\u{EC54}";
pub(crate) const COPY: &str = "\u{E1CA}";
pub(crate) const DUPLICATES: &str = "\u{E710}"; // files
pub(crate) const APPS: &str = "\u{E5DA}"; // app window
pub(crate) const CHART: &str = "\u{E158}"; // chart pie
pub(crate) const TREE: &str = "\u{E67C}";
pub(crate) const LIST: &str = "\u{E2F2}";
pub(crate) const WARNING: &str = "\u{E4E0}";
pub(crate) const CHECK: &str = "\u{E184}"; // check circle
pub(crate) const TRASH: &str = "\u{E4A6}";
pub(crate) const REVEAL: &str = "\u{E5DE}"; // arrow square out
pub(crate) const INFO: &str = "\u{E2CE}";
pub(crate) const SUN: &str = "\u{E472}";
pub(crate) const MOON: &str = "\u{E330}";
pub(crate) const CLOCK: &str = "\u{E1A0}"; // clock counter clockwise
pub(crate) const HOME: &str = "\u{E2C2}";
