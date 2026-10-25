//! Persisted user preferences.

use std::path::Path;

use eframe::Storage;
use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::ui::theme::ThemeMode;
use crate::ui::tokens::{INSPECTOR_WIDTH, NAV_WIDTH};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) theme: ThemeMode,
    pub(crate) language: Language,
    pub(crate) skip_hidden: bool,
    pub(crate) cross_filesystems: bool,
    pub(crate) inspector_open: bool,
    pub(crate) last_path: String,
    pub(crate) recent_roots: Vec<String>,
    pub(crate) sidebar_width: f32,
    pub(crate) inspector_width: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: Language::System,
            skip_hidden: false,
            cross_filesystems: false,
            inspector_open: true,
            last_path: String::new(),
            recent_roots: Vec::new(),
            sidebar_width: NAV_WIDTH,
            inspector_width: INSPECTOR_WIDTH,
        }
    }
}

impl Settings {
    pub(crate) fn load(storage: Option<&dyn Storage>) -> Self {
        storage
            .and_then(|storage| eframe::get_value::<Self>(storage, eframe::APP_KEY))
            .unwrap_or_default()
    }

    pub(crate) fn save(&self, storage: &mut dyn Storage) {
        eframe::set_value(storage, eframe::APP_KEY, self);
    }

    pub(crate) fn remember_root(&mut self, path: &Path) {
        let text = path.display().to_string();
        self.recent_roots.retain(|existing| existing != &text);
        self.recent_roots.insert(0, text);
        self.recent_roots.truncate(8);
    }

    pub(crate) fn reset_preferences(&mut self) {
        let last_path = self.last_path.clone();
        let recent_roots = self.recent_roots.clone();
        *self = Self {
            last_path,
            recent_roots,
            ..Self::default()
        };
    }
}
