//! Small typed localization layer.
//!
//! The GUI only uses typed [`Key`] values. This makes missing translations a
//! compile-time problem instead of a runtime surprise.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub(crate) enum Language {
    #[default]
    System,
    English,
    Russian,
}

impl Language {
    /// Resolve the system preference to a concrete UI language.
    #[must_use]
    pub(crate) fn resolved(self) -> Self {
        match self {
            Self::System => {
                let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
                if locale.starts_with("ru") {
                    Self::Russian
                } else {
                    Self::English
                }
            }
            other => other,
        }
    }

    #[must_use]
    pub(crate) fn is_russian(self) -> bool {
        self.resolved() == Self::Russian
    }

    #[must_use]
    pub(crate) fn label(self, language: Language) -> &'static str {
        match self {
            Self::System => language.text(Key::System),
            Self::English => language.text(Key::English),
            Self::Russian => language.text(Key::Russian),
        }
    }
}

macro_rules! define_text {
    ($($variant:ident => ($en:literal, $ru:literal)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[allow(dead_code)]
        pub(crate) enum Key {
            $($variant),+
        }

        impl Language {
            #[must_use]
            pub(crate) fn text(self, key: Key) -> &'static str {
                match (self.resolved(), key) {
                    $(
                        (Self::English, Key::$variant) => $en,
                        (Self::Russian, Key::$variant) => $ru,
                        (Self::System, Key::$variant) => $en,
                    )+
                }
            }
        }
    };
}

define_text! {
    AppName => ("Disk Cleanup", "Disk Cleanup"),
    Tagline => (
        "Understand what takes space. Clean up safely.",
        "Понятно, что занимает место. Безопасная очистка."
    ),
    Scan => ("Scan", "Сканировать"),
    Cancel => ("Cancel", "Отмена"),
    Browse => ("Browse…", "Выбрать…"),
    Settings => ("Settings", "Настройки"),
    Help => ("Help", "Помощь"),
    PathHint => ("/home/user or C:\\Users", "/home/user или C:\\Users"),
    Ready => ("Ready", "Готово"),
    ScannedIn => ("Scanned in", "Скан за"),
    Scanning => ("Scanning…", "Сканирование…"),
    Cancelling => ("Cancelling…", "Отмена…"),
    ScanFailed => ("Scan failed", "Сканирование не удалось"),
    ScanCancelled => ("Scan cancelled", "Сканирование отменено"),
    Retry => ("Retry", "Повторить"),
    ChooseAnother => ("Choose another folder", "Выбрать другую папку"),
    ChooseFolder => (
        "Choose a folder to scan.",
        "Выберите папку для сканирования."
    ),
    Overview => ("Overview", "Обзор"),
    Tools => ("Tools", "Инструменты"),
    Analyze => ("Analyze", "Анализ"),
    Cleanup => ("Cleanup", "Очистка"),
    Duplicates => ("Duplicates", "Дубликаты"),
    Applications => ("Applications", "Приложения"),
    Soon => ("Soon", "Скоро"),
    Volumes => ("Volumes", "Тома"),
    Refresh => ("Refresh", "Обновить"),
    Recent => ("Recent", "Недавние"),
    NoRecent => (
        "Scanned folders will appear here.",
        "Здесь появятся просканированные папки."
    ),
    Cached => ("cached", "в кэше"),
    Free => ("free", "свободно"),
    Of => ("of", "из"),
    Used => ("used", "занято"),
    DoubleClickScan => (
        "Double-click to scan this volume",
        "Дважды щёлкните, чтобы сканировать этот том"
    ),
    NoVolumes => (
        "No mounted volumes found.",
        "Смонтированные тома не найдены."
    ),
    ChooseVolume => ("Choose a volume or folder", "Выберите том или папку"),
    EmptyTitle => (
        "See what is using your disk",
        "Посмотрите, что занимает диск"
    ),
    EmptyBody => (
        "Scans are read-only. Nothing is deleted in this phase.",
        "Сканирование только читает диск. На этом этапе ничего не удаляется."
    ),
    ScanHome => ("Scan home folder", "Сканировать домашнюю папку"),
    LastScan => ("Last scan", "Последний скан"),
    Path => ("Path", "Путь"),
    Message => ("Message", "Сообщение"),
    Share => ("Share", "Доля"),
    Size => ("Size", "Размер"),
    Name => ("Name", "Имя"),
    Modified => ("Modified", "Изменён"),
    Kind => ("Kind", "Тип"),
    Files => ("Files", "Файлы"),
    Folders => ("Folders", "Папки"),
    OnDisk => ("On disk", "На диске"),
    Logical => ("Logical", "Логический"),
    Actual => ("Actual", "Фактический"),
    Issues => ("Issues", "Проблемы"),
    Map => ("Map", "Карта"),
    TopFiles => ("Files", "Файлы"),
    TopFolders => ("Folders", "Папки"),
    Types => ("Types", "Типы"),
    SearchHint => ("Search name or path…", "Поиск по имени или пути…"),
    Filters => ("Filters", "Фильтры"),
    MinSize => ("Min size", "Мин. размер"),
    Show => ("Show", "Показывать"),
    All => ("All", "Все"),
    Clear => ("Clear", "Сбросить"),
    ClearFilters => ("Clear filters", "Сбросить фильтры"),
    NoMatches => (
        "No entries match the filters.",
        "Нет элементов, подходящих под фильтры."
    ),
    NoFiles => (
        "No files match the filters.",
        "Нет файлов, подходящих под фильтры."
    ),
    NoFolders => (
        "No folders match the filters.",
        "Нет папок, подходящих под фильтры."
    ),
    NoIssues => ("No scan issues.", "Проблем при сканировании нет."),
    NoTypes => ("No files found.", "Файлы не найдены."),
    Selection => ("Selection", "Выбранный объект"),
    NoSelection => (
        "Scan a folder to inspect its largest entries.",
        "Просканируйте папку, чтобы изучить крупнейшие элементы."
    ),
    Up => ("Up", "Вверх"),
    Open => ("Open", "Открыть"),
    Reveal => ("Show in file manager", "Показать в проводнике"),
    CopyPath => ("Copy path", "Скопировать путь"),
    CopiedPath => ("Path copied", "Путь скопирован"),
    RevealFailed => (
        "Could not open the file manager",
        "Не удалось открыть проводник"
    ),
    File => ("File", "Файл"),
    Folder => ("Folder", "Папка"),
    Symlink => ("Symlink", "Симлинк"),
    Other => ("Other", "Другое"),
    Hardlink => ("Hardlink duplicate", "Дубликат жёсткой ссылки"),
    HardlinkNote => (
        "This path is another hardlink and is not counted again.",
        "Это ещё одна жёсткая ссылка; повторно она не учитывается."
    ),
    Children => ("Largest children", "Крупнейшие дочерние"),
    PercentOfParent => ("of parent", "от родителя"),
    Unknown => ("unknown", "неизвестно"),
    JustNow => ("just now", "только что"),
    SecondsAgo => ("s ago", "с назад"),
    MinutesAgo => ("m ago", "мин назад"),
    HoursAgo => ("h ago", "ч назад"),
    DaysAgo => ("d ago", "дн назад"),
    ShowAll => ("Show all", "Показать все"),
    Showing => ("Showing", "Показано"),
    General => ("General", "Общие"),
    ScanningOptions => ("Scanning", "Сканирование"),
    Theme => ("Theme", "Тема"),
    Language => ("Language", "Язык"),
    System => ("System", "Системная"),
    Dark => ("Dark", "Тёмная"),
    Light => ("Light", "Светлая"),
    English => ("English", "English"),
    Russian => ("Russian", "Русский"),
    SkipHidden => ("Skip hidden entries", "Пропускать скрытые элементы"),
    CrossFilesystems => (
        "Cross filesystem boundaries",
        "Переходить границы файловых систем"
    ),
    ScanOptionsNote => (
        "Changes apply to the next scan.",
        "Изменения применяются при следующем сканировании."
    ),
    ShowInspector => ("Show inspector", "Показывать инспектор"),
    Close => ("Close", "Закрыть"),
    Reset => ("Reset", "Сбросить"),
    ResetSettings => ("Reset preferences", "Сбросить настройки"),
    Version => ("Version", "Версия"),
    Shortcuts => ("Keyboard shortcuts", "Горячие клавиши"),
    SafetyNote => (
        "Scanning is read-only. Cleanup moves files to the OS trash only after confirmation.",
        "Сканирование только читает диск. Очистка перемещает файлы в корзину только после подтверждения."
    ),
    Source => ("Source code", "Исходный код"),
    License => ("License", "Лицензия"),
    ClearSearch => ("Clear search", "Очистить поиск"),
    MinSizeHint => ("0, 10M, 1.5GiB", "0, 10M, 1.5GiB"),
    InvalidSize => (
        "Use 0, 10M, 1.5GiB, …",
        "Используйте 0, 10M, 1.5GiB, …"
    ),
    NoTree => ("No scan yet", "Скана ещё нет"),
    Root => ("Root", "Корень"),
    Current => ("Current", "Текущий"),
    Percent => ("%", "%"),
    ShortcutOpen => ("Choose a folder", "Выбрать папку"),
    ShortcutRescan => ("Rescan", "Повторить скан"),
    ShortcutSearch => ("Focus search", "Перейти к поиску"),
    ShortcutBack => ("Go up one folder", "На уровень выше"),
    ShortcutSettings => ("Open settings", "Открыть настройки"),
    ShortcutClose => ("Close dialog / clear focus", "Закрыть диалог / снять фокус"),
    ShortcutHelp => ("Show shortcuts", "Показать горячие клавиши"),
    ScanSummary => (
        "files · {issues} scan issues · in {duration}",
        "файлов · проблем: {issues} · за {duration}"
    ),
    SettingsSaved => ("Preferences saved", "Настройки сохранены"),
    ApplicationSubtitle => (
        "Safe disk analysis and cleanup",
        "Безопасный анализ и очистка диска"
    ),
}

/// Russian plural rules for whole numbers.
fn ru_plural(n: u64, one: &'static str, few: &'static str, many: &'static str) -> &'static str {
    let mod100 = n % 100;
    let mod10 = n % 10;
    if (11..=14).contains(&mod100) {
        many
    } else if mod10 == 1 {
        one
    } else if (2..=4).contains(&mod10) {
        few
    } else {
        many
    }
}

#[must_use]
pub(crate) fn files_text(language: Language, n: u64) -> String {
    match language.resolved() {
        Language::Russian => format!("{n} {}", ru_plural(n, "файл", "файла", "файлов")),
        _ => format!("{n} {}", if n == 1 { "file" } else { "files" }),
    }
}

#[must_use]
pub(crate) fn folders_text(language: Language, n: u64) -> String {
    match language.resolved() {
        Language::Russian => format!("{n} {}", ru_plural(n, "папка", "папки", "папок")),
        _ => format!("{n} {}", if n == 1 { "folder" } else { "folders" }),
    }
}

#[must_use]
pub(crate) fn issues_text(language: Language, n: u64) -> String {
    match language.resolved() {
        Language::Russian => format!("{n} {}", ru_plural(n, "проблема", "проблемы", "проблем")),
        _ => format!("{n} {}", if n == 1 { "issue" } else { "issues" }),
    }
}
