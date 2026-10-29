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
    RefreshApplications => ("Refresh list", "Обновить список"),
    SteamAutomaticSource => ("Automatic source detection", "Автопоиск источников"),
    SteamCustomSource => ("Selected application source", "Выбранный источник приложений"),
    SteamLibrarySettings => ("Application sources…", "Источники приложений…"),
    ApplicationGames => ("Games and applications", "Игры и приложения"),
    ApplicationComponents => ("Steam components", "Компоненты Steam"),
    ApplicationComponentsHint => ("Proton, Steam Linux Runtime and shared redistributables, grouped by their local names.", "Proton, Steam Linux Runtime и общие библиотеки, сгруппированные по локальным названиям."),
    ApplicationPartialSizeHelp => ("Some entries were skipped, for example links, mount points or unreadable files. The measured size may be lower than the full installation size.", "Часть записей пропущена: например, ссылки, точки монтирования или недоступные файлы. Измеренный размер может быть меньше полного размера установки."),
    ApplicationOpenDetails => ("View details →", "Подробнее →"),
    ApplicationsNoMatches => ("No matches. Try a different search.", "Совпадений нет. Измените поисковый запрос."),
    NoApplications => ("No applications found. Check the folder and any read issues.", "Приложения не найдены. Проверьте выбранную папку и ошибки чтения."),
    ApplicationsDescription => ("Launcher games and applications, measured locally.", "Игры и приложения лаунчеров: размер по данным сканирования."),
    SteamRootHint => ("Steam or SteamLibrary folder; leave empty to detect", "Папка Steam или SteamLibrary; пусто — автоопределение"),
    DiscoverApplications => ("Find applications", "Найти приложения"),
    ApplicationsEmpty => ("Search common launcher locations or choose a source folder.", "Выполните поиск в стандартных папках лаунчеров или выберите папку источника."),
    InventoryFailed => ("Application discovery failed. Previous results are preserved.", "Не удалось завершить поиск приложений. Предыдущие результаты сохранены."),
    InventoryCancelled => ("Search cancelled. Previous results are preserved.", "Поиск отменён. Предыдущие результаты сохранены."),
    InventoryIssue => ("Could not read or skipped", "Не удалось прочитать или пропущено"),
    ApplicationInstalled => ("Installed", "Установлено"),
    ApplicationMissing => ("Installation folder missing", "Папка установки отсутствует"),
    ApplicationUnknown => ("Installation status uncertain", "Статус установки не определён"),
    ApplicationPartialSize => ("(partial size)", "(неполный размер)"),
    EpicRootHint => ("Epic Manifests folder (.item files); empty searches all sources", "Папка Manifests Epic (файлы .item); пусто — поиск по всем источникам"),
    LegendaryRootHint => ("Legendary config folder containing installed.json; empty searches all sources", "Папка конфигурации Legendary с installed.json; пусто — поиск по всем источникам"),
    GogRootHint => ("GOG game folder or library with goggame-*.info; empty searches automatic sources", "Папка игры или библиотеки GOG с goggame-*.info; пусто — автоматический поиск"),
    HeroicGogRootHint => ("Heroic config root containing gog_store/installed.json; empty searches all sources", "Корневая папка конфигурации Heroic с gog_store/installed.json; пусто — поиск по всем источникам"),
    ApplicationRuntimes => ("Shared runtimes and components", "Общие среды и компоненты"),
    ApplicationRuntimesHint => ("Shared components are listed separately. Their space is not added to every application; revisions and branches may have separate records.", "Общие компоненты показаны отдельно. Их размер не прибавляется к каждому приложению; ревизии и ветки могут иметь отдельные записи."),
    SnapRootHint => ("snapd data folder containing state.json and snaps/; empty searches automatic sources", "Папка данных snapd со state.json и snaps/; пусто — автоматический поиск"),
    SnapRuntime => ("Snap system component", "Системный компонент Snap"),
    SnapArchiveSize => ("Compressed archive on disk", "Сжатый архив на диске"),
    SnapArchivePath => ("Archive file", "Файл архива"),
    SnapArchiveMissing => ("Archive missing", "Архив отсутствует"),
    SnapRevision => ("Revision", "Ревизия"),
    SnapCurrentEnabled => ("Current, enabled", "Текущая, включена"),
    SnapCurrentDisabled => ("Current, disabled", "Текущая, отключена"),
    SnapRetained => ("Retained revision", "Сохранённая ревизия"),
    SnapStateUnknown => ("State unknown", "Статус неизвестен"),
    SnapOwnership => ("The size measures the compressed .snap archive, excluding mounted /snap content, writable data, caches and snapshots. Local snapd state identifies current and retained revisions when readable; an archive alone does not prove installation or activation. System components are listed separately; content providers and dependency ownership are not inferred. Hard links may share this storage; the size is not a cleanup saving. Retained revisions are not cleanup recommendations. Manage removal through Snap; this screen does not uninstall or select files for cleanup.", "Размер измерен для сжатого архива .snap, без смонтированного содержимого /snap, изменяемых данных, кэшей и снимков. Доступная локальная база snapd определяет текущие и сохранённые ревизии; один архив не подтверждает установку или активацию. Системные компоненты показаны отдельно; поставщики содержимого и принадлежность зависимостей не определяются. Жёсткие ссылки могут разделять это место; размер не равен объёму для освобождения. Сохранённые ревизии не являются рекомендацией к очистке. Удаление — через Snap; этот экран не удаляет приложения и не выбирает файлы для очистки."),
    FlatpakRuntime => ("Flatpak runtime", "Среда Flatpak"),
    FlatpakRootHint => ("Flatpak installation folder containing app/ and runtime/; empty searches automatic sources", "Папка установки Flatpak с app/ и runtime/; пусто — автоматический поиск"),
    FlatpakDeploymentSize => ("Deployment size", "Размер развёртывания"),
    FlatpakRef => ("Reference / architecture / branch", "Идентификатор / архитектура / ветка"),
    FlatpakOwnership => ("Identified from local Flatpak deployment metadata and its active marker. This measures the deployment folder, excluding separate user data, the repository and other runtimes. Hard links may share storage with the repository or other deployments: sizes cannot be added or treated as reclaimable space. Manage uninstalling through Flatpak.", "Определено по локальным метаданным Flatpak и маркеру активного развёртывания. Измерена папка развёртывания без отдельных пользовательских данных, репозитория и других сред. Жёсткие ссылки могут разделять место с репозиторием и другими развёртываниями: размеры нельзя суммировать или считать объёмом для освобождения. Удаление — через Flatpak."),
    ApplicationPackages => ("System packages", "Системные пакеты"),
    ApplicationPackagesHint => ("Installed packages include libraries and dependencies. Manage removal through the package manager.", "Установленные пакеты включают библиотеки и зависимости. Удаление — через пакетный менеджер."),
    PackageReportedSize => ("Package estimate", "Оценка пакета"),
    PackageOwnership => ("Identified from the local installed-package database. The size is reported by the package manager; it is not measured disk usage or reclaimable space. It excludes separate user data and does not account for shared dependencies. Uninstall through the package manager.", "Определено по локальной базе установленных пакетов. Размер указан пакетным менеджером: это не измеренное место на диске и не объём для освобождения. Отдельные пользовательские данные и общие зависимости в нём не учтены. Удаление — через пакетный менеджер."),
    PackageNoInstallPath => ("Package files are spread across system directories; there is no single installation folder to analyze.", "Файлы пакета распределены по системным каталогам; единой папки установки для анализа нет."),
    PackageVersion => ("Version", "Версия"),
    PackageIdentity => ("Package / architecture", "Пакет / архитектура"),
    DpkgRootHint => ("dpkg database folder containing status; empty searches automatic sources", "Папка базы dpkg с файлом status; пусто — автоматический поиск"),
    PacmanRootHint => ("pacman local database folder (with package/desc files); empty searches automatic sources", "Папка локальной базы pacman (с файлами пакет/desc); пусто — автоматический поиск"),
    RegistrySource => ("Windows registry", "Реестр Windows"),
    RegistryRootHint => ("Reads installed-program records for the current user and machine (Windows). No source folder is needed.", "Читает записи программ текущего пользователя и компьютера (Windows). Папка источника не нужна."),
    RegistryOwnership => ("Identified from Windows Uninstall records. A registry entry does not prove file integrity or exclusive folder ownership. Manage uninstalling in Windows Settings or the original installer.", "Определено по записям Uninstall в реестре Windows. Запись не подтверждает целостность файлов или исключительное владение папкой. Удаление — через параметры Windows или исходный установщик."),
    ApplicationNoInstallPath => ("Installation folder is not recorded or cannot be used safely. Size is unknown.", "Папка установки не указана или не подходит для безопасного сканирования. Размер неизвестен."),
    ApplicationEvidence => ("Source record", "Запись источника"),
    LutrisRootHint => ("Lutris data folder containing pga.db (Linux); empty searches automatic sources", "Папка данных Lutris с pga.db (Linux); пусто — автоматический поиск"),
    LutrisOwnership => ("Identified from Lutris records marked installed. Size covers the recorded directory, which may contain a Wine prefix or shared files. This is not content-integrity verification. Uninstall through Lutris.", "Определено по записям Lutris с признаком установки. Размер относится к указанной папке: в ней могут быть Wine-префикс и общие файлы. Это не проверка целостности. Удаление — через Lutris."),
    GogOwnership => ("Identified from local GOG installation info. This is metadata evidence, not an integrity check. Uninstall through GOG or its installer.", "Определено по локальному файлу установки GOG. Это данные установки, а не проверка целостности. Удаление — через GOG или установщик."),
    HeroicGogOwnership => ("Identified from Heroic's installed GOG store. Name comes from local GOG info or the installation folder. A resume marker means installation status is uncertain. Uninstall through Heroic.", "Определено по списку установленных игр GOG в Heroic. Название взято из локальных данных GOG или имени папки. Маркер незавершённой загрузки делает статус неопределённым. Удаление — через Heroic."),
    LegendaryOwnership => ("Identified from local Legendary installed-game records (also used by Heroic for Epic games). Manage uninstalling in that launcher. Heroic GOG/Amazon games are separate sources.", "Определено по локальным записям установок Legendary (также используются Heroic для игр Epic). Удаление — через соответствующий лаунчер. Игры GOG/Amazon в Heroic — отдельные источники."),
    EpicOwnership => ("Identified from a local Epic manifest. Manage uninstalling in Epic Games Launcher.", "Определено по локальному манифесту Epic. Удаление игры — через Epic Games Launcher."),
    SteamOwnership => ("Identified from a local Steam app manifest. Manage uninstalling in Steam.", "Определено по локальному манифесту Steam. Удаление игры — через Steam."),
    ApplicationDataScope => ("Size covers only the recorded installation directory; files inside it are included. External saves, prefixes and shader caches are excluded. Shared files may appear under multiple apps; sizes are not added together.", "Размер включает указанную папку установки и все файлы внутри неё. Внешние сохранения, префиксы и кэш шейдеров не включены. Общие файлы могут относиться к нескольким приложениям; размеры не суммируются."),
    ApplicationBuild => ("Build", "Сборка"),
    ApplicationLastPlayed => ("Last played", "Последний запуск"),
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
    ScanProgressEstimate => (
        "Scanned file bytes / occupied space on the volume. Approximate: other folders, skipped files and filesystem metadata affect the total.",
        "Объём проверенных файлов / занятое место на диске. Оценка приблизительная: в общий объём входят другие папки, пропущенные файлы и данные файловой системы."
    ),
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
    WorkerThreads => ("Worker threads", "Число потоков"),
    AutomaticThreads => ("Automatic", "Автоматически"),
    WorkerThreadsNote => (
        "For scanning and duplicates. Automatic uses up to 8 workers. Use 1 for sequential processing; fewer workers may help on HDDs. Applies to new jobs.",
        "Для сканирования и дубликатов. Автоматически — до 8 потоков. 1 — последовательная обработка; для HDD может быть лучше меньше потоков. Применяется к новым задачам."
    ),
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
    ShortcutOpenItem => ("Open selected item", "Открыть выбранный элемент"),
    ShortcutWorkspaces => (
        "Switch Analyze / Cleanup / Duplicates",
        "Переключить Анализ / Очистку / Дубликаты"
    ),
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
    CleanupSubtitle => (
        "Review recommendations before moving anything to the trash",
        "Проверьте рекомендации перед перемещением в корзину"
    ),
    CleanupDescription => (
        "Disk Cleanup checks known cache and temporary locations. Nothing is selected by default, and only reviewed items are moved to the operating-system trash.",
        "Disk Cleanup проверяет известные места кэша и временных файлов. По умолчанию ничего не выбрано, а в корзину перемещаются только проверенные элементы."
    ),
    FindRecommendations => ("Find recommendations", "Найти рекомендации"),
    ScanAgain => ("Scan again", "Сканировать снова"),
    ScanningRecommendations => (
        "Checking cleanup locations…",
        "Проверяем места очистки…"
    ),
    RecommendationsFailed => (
        "Could not collect cleanup recommendations",
        "Не удалось собрать рекомендации по очистке"
    ),
    DiscoveryStopped => (
        "Cleanup discovery stopped unexpectedly",
        "Поиск рекомендаций неожиданно остановился"
    ),
    ScannerStopped => (
        "The scanner stopped unexpectedly",
        "Сканер неожиданно остановился"
    ),
    CleanupWorkerStopped => (
        "The cleanup worker stopped unexpectedly",
        "Обработчик очистки неожиданно остановился"
    ),
    DuplicateWorkerStopped => (
        "The duplicate cleanup worker stopped unexpectedly",
        "Обработчик удаления дубликатов неожиданно остановился"
    ),
    NoCleanupCandidates => (
        "No cleanup candidates found",
        "Кандидаты для очистки не найдены"
    ),
    NoCleanupCandidatesBody => (
        "The known cache and temporary locations are empty or already clean. System commands below are still recommendation-only.",
        "Известные места кэша и временных файлов пусты или уже очищены. Системные команды ниже по-прежнему носят рекомендательный характер."
    ),
    CheckedAt => ("Checked", "Проверено"),
    PotentialSavings => ("Potential savings", "Можно освободить"),
    Candidates => ("Candidates", "Кандидаты"),
    Selected => ("Selected", "Выбрано"),
    Safe => ("Safe", "Безопасные"),
    SelectSafe => ("Select safe", "Выбрать безопасные"),
    ClearSelection => ("Clear selection", "Снять выбор"),
    NeedsConfirmation => ("Requires confirmation", "Требует подтверждения"),
    HighRiskSelected => (
        "High-risk items are selected",
        "Выбраны элементы высокого риска"
    ),
    ReviewBasket => ("Review basket", "Корзина проверки"),
    ReviewBasketHint => (
        "Select items above to review them together.",
        "Выберите элементы выше, чтобы проверить их вместе."
    ),
    MoveToTrash => ("Move to Trash…", "Переместить в корзину…"),
    MoveToTrashTitle => (
        "Move selected items to the trash?",
        "Переместить выбранные элементы в корзину?"
    ),
    MoveToTrashBody => (
        "The items will be moved to the operating-system trash after final checks. Nothing is permanently deleted by Disk Cleanup.",
        "После финальной проверки элементы будут перемещены в корзину операционной системы. Disk Cleanup ничего не удаляет безвозвратно."
    ),
    HighRiskAck => (
        "I reviewed the high-risk items and want to move them to the trash.",
        "Я проверил элементы высокого риска и хочу переместить их в корзину."
    ),
    AuditLog => ("Audit log", "Журнал аудита"),
    OpenAuditLog => ("Open audit log", "Открыть журнал аудита"),
    OpenTrash => ("Open trash", "Открыть корзину"),
    AuditUnavailable => (
        "The audit log location is unavailable, so cleanup is disabled.",
        "Расположение журнала аудита недоступно, поэтому очистка отключена."
    ),
    Applying => ("Moving to the trash…", "Перемещаем в корзину…"),
    IssuePermission => ("Access denied", "Нет доступа"),
    IssueSearchPaths => ("Search paths; size/type filters apply to data views.", "Поиск по пути; размер и тип относятся к данным."),
    IssueMissing => ("Not found", "Не найдено"),
    IssueReadDirectory => ("Directory", "Чтение папки"),
    IssueMetadata => ("Metadata", "Метаданные"),
    IssuePermissionBody => ("This item could not be read with your permissions.", "Недостаточно прав для чтения объекта."),
    IssueMissingBody => ("This item disappeared during the scan.", "Объект исчез во время сканирования."),
    IssueReadBody => ("Could not read this item. Hover for technical details.", "Не удалось прочитать объект. Подробности — при наведении."),
    CancelAfterCurrent => ("Cancellation waits for the current OS operation.", "Отмена дождётся завершения текущей системной операции."),
    ApplyComplete => ("Moved to the trash", "Перемещено в корзину"),
    ApplyPartial => (
        "Cleanup finished with some failures",
        "Очистка завершена с ошибками"
    ),
    ApplyFailed => (
        "Cleanup could not start",
        "Не удалось начать очистку"
    ),
    NothingPermanentlyDeleted => (
        "Trash contents can be restored until the trash is emptied.",
        "Содержимое корзины можно восстановить, пока корзина не очищена."
    ),
    SystemCommands => ("System commands", "Системные команды"),
    CommandOnly => ("Recommendation only", "Только рекомендация"),
    RequiresAdmin => (
        "Requires administrator rights",
        "Требуются права администратора"
    ),
    CopyCommand => ("Copy command", "Скопировать команду"),
    CopiedCommand => ("Command copied", "Команда скопирована"),
    SystemCommandsNote => (
        "Disk Cleanup never runs these commands. Review and run them yourself in a terminal if needed.",
        "Disk Cleanup никогда не выполняет эти команды. При необходимости проверьте и запустите их самостоятельно в терминале."
    ),
    ShowMore => ("Show more", "Показать ещё"),
    ShowLess => ("Show less", "Скрыть"),
    RiskLow => ("Low risk", "Низкий риск"),
    RiskMedium => ("Review", "Проверить"),
    RiskHigh => ("High risk", "Высокий риск"),
    RiskSystem => ("Command only", "Только команда"),
    CategoryTemporary => ("Temporary", "Временные файлы"),
    CategoryAppCache => ("App cache", "Кэш приложений"),
    CategoryPackageCache => ("Package cache", "Кэш пакетов"),
    CategoryTrash => ("Trash", "Корзина"),
    CategoryOldDownloads => ("Old downloads", "Старые загрузки"),
    CategoryLargeOldFiles => ("Large old files", "Крупные старые файлы"),
    CategoryDevArtifacts => ("Build artifacts", "Артефакты сборки"),
    CategoryDuplicates => ("Duplicates", "Дубликаты"),
    CategorySystem => ("System", "Система"),
    CleanupIssues => ("Skipped paths", "Пропущенные пути"),
    SelectItemsHint => (
        "Nothing is selected. Review candidates and choose what to remove.",
        "Ничего не выбрано. Проверьте кандидатов и выберите, что удалить."
    ),
    FailedItems => ("failed", "с ошибкой"),
    Dismiss => ("Dismiss", "Закрыть"),
    DuplicatesSubtitle => (
        "Find identical files and free space safely",
        "Находите одинаковые файлы и освобождайте место безопасно"
    ),
    DuplicatesDescription => (
        "Exact duplicates are verified by size, a content prefix, and a full BLAKE3 hash. Nothing is selected automatically.",
        "Точные дубликаты проверяются по размеру, префиксу содержимого и полному хешу BLAKE3. Ничего не выбирается автоматически."
    ),
    FindDuplicates => ("Find duplicates", "Найти дубликаты"),
    ScanningDuplicates => (
        "Hashing candidate files…",
        "Хешируем файлы-кандидаты…"
    ),
    DuplicatesFailed => (
        "Could not search for duplicates",
        "Не удалось найти дубликаты"
    ),
    DuplicateSearchStopped => (
        "The duplicate search stopped unexpectedly",
        "Поиск дубликатов неожиданно остановился"
    ),
    NoDuplicates => ("No exact duplicates found", "Точные дубликаты не найдены"),
    NoDuplicatesBody => (
        "Try a lower minimum size or scan a different folder.",
        "Попробуйте уменьшить минимальный размер или выбрать другую папку."
    ),
    DuplicateGroups => ("Duplicate groups", "Группы дубликатов"),
    Copies => ("copies", "копий"),
    Each => ("each", "каждый"),
    Reclaimable => ("Reclaimable", "Можно освободить"),
    Keep => ("Keep", "Оставить"),
    Keeper => ("Keeper", "Оригинал"),
    SelectRest => ("Select the rest", "Выбрать остальные"),
    SelectAllExceptKeepers => (
        "Select all except keepers",
        "Выбрать все, кроме оригиналов"
    ),
    DuplicatesReviewNote => (
        "Pick one keeper per group. The other copies can be moved to the trash through the same audited pipeline.",
        "Выберите оригинал в каждой группе. Остальные копии можно переместить в корзину через тот же журналируемый процесс."
    ),
    DuplicatesNeedScan => (
        "Scan a folder first",
        "Сначала просканируйте папку"
    ),
    DuplicatesNeedScanBody => (
        "Duplicate search uses the folder from the current analysis scan. Scan a folder to begin.",
        "Поиск дубликатов использует папку текущего анализа. Просканируйте папку, чтобы начать."
    ),
    HardlinksSkipped => (
        "hardlink alias(es) skipped",
        "алиас(ов) жёстких ссылок пропущено"
    ),
    HashedBytes => ("hashed", "прохешировано"),
    ShowAllCopies => ("Show all copies", "Показать все копии"),
    DuplicateTrashBody => (
        "Selected extra copies will be moved to the trash. Keepers and unselected files stay in place.",
        "Выбранные лишние копии будут перемещены в корзину. Оригиналы и невыбранные файлы останутся на месте."
    ),
    DuplicatesBasketHint => (
        "Select extra copies above. Keepers are never selected automatically.",
        "Выберите лишние копии выше. Оригиналы никогда не выбираются автоматически."
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

#[must_use]
pub(crate) fn items_text(language: Language, n: u64) -> String {
    match language.resolved() {
        Language::Russian => format!("{n} {}", ru_plural(n, "элемент", "элемента", "элементов")),
        _ => format!("{n} {}", if n == 1 { "item" } else { "items" }),
    }
}

#[must_use]
pub(crate) fn groups_text(language: Language, n: u64) -> String {
    match language.resolved() {
        Language::Russian => format!("{n} {}", ru_plural(n, "группа", "группы", "групп")),
        _ => format!("{n} {}", if n == 1 { "group" } else { "groups" }),
    }
}

#[must_use]
pub(crate) fn copies_text(language: Language, n: u64) -> String {
    match language.resolved() {
        Language::Russian => format!("{n} {}", ru_plural(n, "копия", "копии", "копий")),
        _ => format!("{n} {}", if n == 1 { "copy" } else { "copies" }),
    }
}

#[must_use]
pub(crate) fn hardlinks_text(language: Language, n: u64) -> String {
    if language.is_russian() {
        format!("жёстких ссылок пропущено: {n}")
    } else {
        format!("hardlink aliases skipped: {n}")
    }
}
