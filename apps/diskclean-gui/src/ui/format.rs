//! Display formatting shared by the shell and views.

use std::time::{SystemTime, UNIX_EPOCH};

use diskclean_core::format_bytes;

use crate::i18n::{Key, Language};

#[must_use]
pub(crate) fn size(language: Language, bytes: u64) -> String {
    let text = format_bytes(bytes);
    if language.is_russian() {
        text.replace('.', ",")
    } else {
        text
    }
}

#[must_use]
pub(crate) fn duration(language: Language, milliseconds: u64) -> String {
    if milliseconds < 1000 {
        if language.is_russian() {
            format!("{milliseconds} мс")
        } else {
            format!("{milliseconds} ms")
        }
    } else if language.is_russian() {
        format!("{:.1} с", milliseconds as f64 / 1000.0).replace('.', ",")
    } else {
        format!("{:.1} s", milliseconds as f64 / 1000.0)
    }
}

#[must_use]
pub(crate) fn percent(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64 * 100.0).clamp(0.0, 100.0) as f32
    }
}

#[must_use]
pub(crate) fn age(language: Language, unix_seconds: i64) -> String {
    if unix_seconds <= 0 {
        return language.text(Key::Unknown).to_owned();
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let then = u64::try_from(unix_seconds).unwrap_or(0);
    if then >= now {
        return language.text(Key::JustNow).to_owned();
    }

    let seconds = now - then;
    if seconds < 60 {
        return if language.is_russian() {
            format!("{seconds} с назад")
        } else {
            format!("{seconds}s ago")
        };
    }
    if seconds < 3600 {
        let n = seconds / 60;
        return if language.is_russian() {
            format!("{n} мин назад")
        } else {
            format!("{n}m ago")
        };
    }
    if seconds < 86_400 {
        let n = seconds / 3600;
        return if language.is_russian() {
            format!("{n} ч назад")
        } else {
            format!("{n}h ago")
        };
    }
    let n = seconds / 86_400;
    if language.is_russian() {
        format!("{n} дн назад")
    } else {
        format!("{n}d ago")
    }
}

/// Shorten a display string in the middle, preserving the file/folder name.
#[must_use]
pub(crate) fn shorten_middle(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars || max_chars < 5 {
        return text.to_owned();
    }

    let keep = max_chars.saturating_sub(3);
    let head = keep / 2;
    let tail = keep - head;
    let head_text: String = text.chars().take(head).collect();
    let tail_text: String = text
        .chars()
        .rev()
        .take(tail)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head_text}…{tail_text}")
}

#[must_use]
pub(crate) fn file_name(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
