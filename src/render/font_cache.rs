//! Persistent index of system font metadata.
//!
//! `FontSystem::new` asks fontdb to open and parse every installed font, which
//! cosmic-text documents as taking up to a second in release builds. A warm
//! start only needs the metadata (family names, style and source path), so this
//! module stores that index and rebuilds an equivalent `fontdb::Database`
//! without reading the font binaries. Any miss or error returns `None` and the
//! caller falls back to the regular system scan.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use glyphon::FontSystem;
use glyphon::fontdb;
use serde::{Deserialize, Serialize};

use crate::config;

/// Bump when the on-disk shape or reconstruction rules change.
const CACHE_VERSION: u32 = 2;
const CACHE_FILE_NAME: &str = "font_index.json";
/// Anything bigger than this is ignored instead of being parsed.
const MAX_CACHE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

/// Serialized metadata for every face in a `fontdb::Database`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FontIndex {
    version: u32,
    app_version: String,
    platform: String,
    built_at_secs: u64,
    faces: Vec<CachedFace>,
    dirs: Vec<CachedDir>,
    #[serde(default)]
    generic: GenericFamilies,
}

/// Fontconfig can remap the generic CSS families (for example, the user's
/// preferred monospace font). Preserve those mappings across warm starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct GenericFamilies {
    serif: String,
    sans_serif: String,
    cursive: String,
    fantasy: String,
    monospace: String,
}

impl GenericFamilies {
    fn capture(database: &fontdb::Database) -> Self {
        let name = |family| database.family_name(&family).to_owned();
        Self {
            serif: name(fontdb::Family::Serif),
            sans_serif: name(fontdb::Family::SansSerif),
            cursive: name(fontdb::Family::Cursive),
            fantasy: name(fontdb::Family::Fantasy),
            monospace: name(fontdb::Family::Monospace),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedFace {
    path: PathBuf,
    index: u32,
    families: Vec<String>,
    post_script_name: String,
    style: CachedStyle,
    weight: u16,
    stretch: CachedStretch,
    monospaced: bool,
    file_size: u64,
    file_mtime_ns: u64,
}

/// A parent directory of one or more font files. Its mtime changes when fonts
/// are added to or removed from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CachedDir {
    path: PathBuf,
    mtime_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CachedStyle {
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CachedStretch {
    UltraCondensed,
    ExtraCondensed,
    Condensed,
    SemiCondensed,
    Normal,
    SemiExpanded,
    Expanded,
    ExtraExpanded,
    UltraExpanded,
}

impl From<fontdb::Style> for CachedStyle {
    fn from(style: fontdb::Style) -> Self {
        match style {
            fontdb::Style::Normal => Self::Normal,
            fontdb::Style::Italic => Self::Italic,
            fontdb::Style::Oblique => Self::Oblique,
        }
    }
}

impl From<CachedStyle> for fontdb::Style {
    fn from(style: CachedStyle) -> Self {
        match style {
            CachedStyle::Normal => Self::Normal,
            CachedStyle::Italic => Self::Italic,
            CachedStyle::Oblique => Self::Oblique,
        }
    }
}

impl From<fontdb::Stretch> for CachedStretch {
    fn from(stretch: fontdb::Stretch) -> Self {
        match stretch {
            fontdb::Stretch::UltraCondensed => Self::UltraCondensed,
            fontdb::Stretch::ExtraCondensed => Self::ExtraCondensed,
            fontdb::Stretch::Condensed => Self::Condensed,
            fontdb::Stretch::SemiCondensed => Self::SemiCondensed,
            fontdb::Stretch::Normal => Self::Normal,
            fontdb::Stretch::SemiExpanded => Self::SemiExpanded,
            fontdb::Stretch::Expanded => Self::Expanded,
            fontdb::Stretch::ExtraExpanded => Self::ExtraExpanded,
            fontdb::Stretch::UltraExpanded => Self::UltraExpanded,
        }
    }
}

impl From<CachedStretch> for fontdb::Stretch {
    fn from(stretch: CachedStretch) -> Self {
        match stretch {
            CachedStretch::UltraCondensed => Self::UltraCondensed,
            CachedStretch::ExtraCondensed => Self::ExtraCondensed,
            CachedStretch::Condensed => Self::Condensed,
            CachedStretch::SemiCondensed => Self::SemiCondensed,
            CachedStretch::Normal => Self::Normal,
            CachedStretch::SemiExpanded => Self::SemiExpanded,
            CachedStretch::Expanded => Self::Expanded,
            CachedStretch::ExtraExpanded => Self::ExtraExpanded,
            CachedStretch::UltraExpanded => Self::UltraExpanded,
        }
    }
}

/// Try to rebuild a `FontSystem` from the on-disk index.
///
/// Returns `None` for a missing, corrupt, mismatched, stale or incomplete
/// cache. The caller must then perform the regular system font scan.
pub(crate) fn load() -> Option<FontSystem> {
    let path = cache_path()?;
    let started = Instant::now();
    let index = read_index(&path)?;

    let validation_started = Instant::now();
    if let Err(reason) = index.validate(SystemTime::now()) {
        log::debug!(
            "font index cache at {} is unusable: {reason}",
            path.display()
        );
        return None;
    }
    let validation_elapsed = validation_started.elapsed();
    let faces = index.faces.len();
    let dirs = index.dirs.len();
    let database = index.into_database();
    let font_system = FontSystem::new_with_locale_and_db(system_locale(), database);

    log::debug!(
        "font index cache validated {faces} faces and {dirs} directories in {validation_elapsed:?}"
    );
    log::info!(
        "font index cache hit: {faces} faces in {:?}",
        started.elapsed()
    );
    Some(font_system)
}

/// Save the metadata of an already scanned database for the next warm start.
///
/// Failures are expected (read-only cache directory, unsupported font source,
/// unreadable metadata) and are only logged at debug level.
pub(crate) fn store(database: &fontdb::Database) {
    let Some(path) = cache_path() else {
        log::debug!("font index cache disabled: no cache directory");
        return;
    };

    let started = Instant::now();
    let Some(index) = FontIndex::from_database(database) else {
        return;
    };
    let bytes = match serde_json::to_vec(&index) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::debug!("failed to serialize font index cache: {error}");
            return;
        }
    };
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        log::debug!(
            "font index cache would be {} bytes, above the {MAX_CACHE_BYTES} byte limit",
            bytes.len()
        );
        return;
    }

    match write_index(&path, &bytes) {
        Ok(()) => log::debug!(
            "wrote font index cache with {} faces and {} directories ({} bytes) to {} in {:?}",
            index.faces.len(),
            index.dirs.len(),
            bytes.len(),
            path.display(),
            started.elapsed()
        ),
        Err(error) => log::debug!(
            "failed to write font index cache to {}: {error}",
            path.display()
        ),
    }
}

fn cache_path() -> Option<PathBuf> {
    Some(config::cache_dir()?.join(CACHE_FILE_NAME))
}

fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Common font roots, tracked even when empty so adding a font directory
/// invalidates a warm cache. `fontdb` does not expose the roots it scanned.
fn known_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);

    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/AssetsV2"));
        dirs.push(PathBuf::from("/Network/Library/Fonts"));
        if let Some(home) = &home {
            dirs.push(home.join("Library/Fonts"));
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = &home {
            dirs.push(home.join(".fonts"));
            dirs.push(home.join(".local/share/fonts"));
        }
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(data).join("fonts"));
        }
    }

    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SYSTEMROOT") {
            dirs.push(PathBuf::from(root).join("Fonts"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(local)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
        if let Some(roaming) = std::env::var_os("APPDATA") {
            dirs.push(
                PathBuf::from(roaming)
                    .join("Microsoft")
                    .join("Windows")
                    .join("Fonts"),
            );
        }
    }

    dirs
}

fn read_index(path: &Path) -> Option<FontIndex> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            log::debug!(
                "font index cache unavailable at {}: {error}",
                path.display()
            );
            return None;
        }
    };
    if !metadata.is_file() {
        log::debug!("font index cache at {} is not a file", path.display());
        return None;
    }
    if metadata.len() > MAX_CACHE_BYTES {
        log::debug!(
            "font index cache at {} is too large ({} bytes)",
            path.display(),
            metadata.len()
        );
        return None;
    }

    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::debug!(
                "failed to read font index cache at {}: {error}",
                path.display()
            );
            return None;
        }
    };
    if bytes.len() as u64 > MAX_CACHE_BYTES {
        log::debug!(
            "font index cache at {} grew past the size limit while reading",
            path.display()
        );
        return None;
    }

    match serde_json::from_slice(&bytes) {
        Ok(index) => Some(index),
        Err(error) => {
            log::debug!(
                "failed to parse font index cache at {}: {error}",
                path.display()
            );
            None
        }
    }
}

impl FontIndex {
    fn from_database(database: &fontdb::Database) -> Option<Self> {
        let mut faces = Vec::with_capacity(database.len());
        let mut dirs = BTreeSet::new();
        let mut file_metadata: HashMap<PathBuf, (u64, u64)> = HashMap::new();

        for face in database.faces() {
            let path = match &face.source {
                fontdb::Source::File(path) => path.clone(),
                fontdb::Source::SharedFile(path, _) => path.clone(),
                fontdb::Source::Binary(_) => {
                    log::debug!("font index cache disabled: database contains in-memory font data");
                    return None;
                }
            };

            let (file_size, file_mtime_ns) = if let Some(metadata) = file_metadata.get(&path) {
                *metadata
            } else {
                let metadata = match fs::metadata(&path) {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        log::debug!(
                            "font index cache disabled: cannot stat {}: {error}",
                            path.display()
                        );
                        return None;
                    }
                };
                let Some(mtime_ns) = file_mtime_ns(&metadata) else {
                    log::debug!("font index cache disabled: no mtime for {}", path.display());
                    return None;
                };
                let values = (metadata.len(), mtime_ns);
                file_metadata.insert(path.clone(), values);
                values
            };

            if let Some(parent) = path.parent() {
                dirs.insert(parent.to_path_buf());
            }

            faces.push(CachedFace {
                path,
                index: face.index,
                families: face.families.iter().map(|(name, _)| name.clone()).collect(),
                post_script_name: face.post_script_name.clone(),
                style: face.style.into(),
                weight: face.weight.0,
                stretch: face.stretch.into(),
                monospaced: face.monospaced,
                file_size,
                file_mtime_ns,
            });
        }

        if faces.is_empty() {
            log::debug!("font index cache disabled: the system scan found no faces");
            return None;
        }

        for path in known_font_dirs() {
            dirs.insert(path);
        }
        let mut cached_dirs = Vec::with_capacity(dirs.len());
        for path in dirs {
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {
                    let Some(mtime_ns) = file_mtime_ns(&metadata) else {
                        log::debug!("font index cache disabled: no mtime for {}", path.display());
                        return None;
                    };
                    cached_dirs.push(CachedDir { path, mtime_ns });
                }
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // Remember that the directory was absent so creating it
                    // later invalidates the cache and picks up new fonts.
                    cached_dirs.push(CachedDir { path, mtime_ns: 0 });
                }
                Err(error) => {
                    log::debug!(
                        "font index cache disabled: cannot stat {}: {error}",
                        path.display()
                    );
                    return None;
                }
            }
        }

        Some(Self {
            version: CACHE_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: current_platform(),
            built_at_secs: unix_secs(SystemTime::now())?,
            faces,
            dirs: cached_dirs,
            generic: GenericFamilies::capture(database),
        })
    }

    fn validate(&self, now: SystemTime) -> Result<(), String> {
        if self.version != CACHE_VERSION {
            return Err(format!(
                "cache version {} does not match {CACHE_VERSION}",
                self.version
            ));
        }
        let app_version = env!("CARGO_PKG_VERSION");
        if self.app_version != app_version {
            return Err(format!(
                "cache app version {} does not match {app_version}",
                self.app_version
            ));
        }
        let platform = current_platform();
        if self.platform != platform {
            return Err(format!(
                "cache platform {} does not match {platform}",
                self.platform
            ));
        }
        if self.faces.is_empty() {
            return Err("cache contains no faces".to_owned());
        }

        let Some(built_at) = UNIX_EPOCH.checked_add(Duration::from_secs(self.built_at_secs)) else {
            return Err("cache timestamp is out of range".to_owned());
        };
        if let Ok(age) = now.duration_since(built_at) {
            if age > MAX_AGE {
                return Err(format!(
                    "cache is {} days old",
                    age.as_secs() / (24 * 60 * 60)
                ));
            }
        } else if let Ok(ahead) = built_at.duration_since(now)
            && ahead > CLOCK_SKEW
        {
            return Err("cache was built in the future".to_owned());
        }

        // Font collections produce multiple faces per file, so only stat each
        // path once. This keeps warm-start validation to a few hundred stats.
        let mut checked_files: HashMap<&Path, (u64, u64)> = HashMap::new();
        for face in &self.faces {
            let path = face.path.as_path();
            let (size, mtime_ns) = if let Some(metadata) = checked_files.get(path) {
                *metadata
            } else {
                let metadata = fs::metadata(path)
                    .map_err(|error| format!("cannot stat {}: {error}", path.display()))?;
                if !metadata.is_file() {
                    return Err(format!("{} is no longer a file", path.display()));
                }
                let mtime_ns = file_mtime_ns(&metadata)
                    .ok_or_else(|| format!("no mtime for {}", path.display()))?;
                let values = (metadata.len(), mtime_ns);
                checked_files.insert(path, values);
                values
            };
            if size != face.file_size {
                return Err(format!(
                    "size of {} changed from {} to {size}",
                    path.display(),
                    face.file_size
                ));
            }
            if mtime_ns != face.file_mtime_ns {
                return Err(format!("mtime of {} changed", path.display()));
            }
        }

        for dir in &self.dirs {
            match fs::metadata(&dir.path) {
                Ok(metadata) if metadata.is_dir() => {
                    if dir.mtime_ns == 0 {
                        return Err(format!("font directory {} appeared", dir.path.display()));
                    }
                    let mtime_ns = file_mtime_ns(&metadata)
                        .ok_or_else(|| format!("no mtime for {}", dir.path.display()))?;
                    if mtime_ns != dir.mtime_ns {
                        return Err(format!("contents of {} changed", dir.path.display()));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && dir.mtime_ns == 0 => {
                    // It was absent when the index was built and still is.
                }
                Err(error) => {
                    return Err(format!("cannot stat {}: {error}", dir.path.display()));
                }
                Ok(_) => {
                    return Err(format!("{} is no longer a directory", dir.path.display()));
                }
            }
        }

        Ok(())
    }

    fn into_database(self) -> fontdb::Database {
        let mut database = fontdb::Database::new();
        for face in self.faces {
            database.push_face_info(fontdb::FaceInfo {
                id: fontdb::ID::dummy(),
                source: fontdb::Source::File(face.path),
                index: face.index,
                families: face
                    .families
                    .into_iter()
                    .map(|name| (name, fontdb::Language::English_UnitedStates))
                    .collect(),
                post_script_name: face.post_script_name,
                style: face.style.into(),
                weight: fontdb::Weight(face.weight),
                stretch: face.stretch.into(),
                monospaced: face.monospaced,
            });
        }

        // Match what cosmic-text does after a regular system scan, preserving
        // any fontconfig aliases captured on the original scan.
        let generic = self.generic;
        database.set_monospace_family(if generic.monospace.is_empty() {
            "Noto Sans Mono".to_owned()
        } else {
            generic.monospace
        });
        database.set_sans_serif_family(if generic.sans_serif.is_empty() {
            "Open Sans".to_owned()
        } else {
            generic.sans_serif
        });
        database.set_serif_family(if generic.serif.is_empty() {
            "DejaVu Serif".to_owned()
        } else {
            generic.serif
        });
        if !generic.cursive.is_empty() {
            database.set_cursive_family(generic.cursive);
        }
        if !generic.fantasy.is_empty() {
            database.set_fantasy_family(generic.fantasy);
        }
        database
    }
}

fn write_index(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let Some(dir) = path.parent() else {
        return Err(std::io::Error::other("cache path has no parent"));
    };
    fs::create_dir_all(dir)?;

    let temp = dir.join(format!(
        ".{CACHE_FILE_NAME}.{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let result = write_temp_and_rename(&temp, path, bytes);
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_temp_and_rename(temp: &Path, destination: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // `mode` is filtered by the umask, so force the exact permission.
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.flush()?;
    drop(file);

    match fs::rename(temp, destination) {
        Ok(()) => Ok(()),
        Err(first_error) => {
            // Windows does not replace an existing destination atomically.
            #[cfg(windows)]
            {
                if destination.exists() {
                    fs::remove_file(destination)?;
                    return fs::rename(temp, destination);
                }
            }
            Err(first_error)
        }
    }
}

fn file_mtime_ns(metadata: &fs::Metadata) -> Option<u64> {
    metadata.modified().ok().and_then(unix_nanos)
}

fn unix_nanos(time: SystemTime) -> Option<u64> {
    u64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_nanos()).ok()
}

fn unix_secs(time: SystemTime) -> Option<u64> {
    Some(time.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// Match `FontSystem::new`'s locale detection so warm starts shape fallback
/// glyphs exactly like a cold scan. The direct dependency is already pulled in
/// by cosmic-text.
fn system_locale() -> String {
    sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned())
}

#[cfg(test)]
fn parse_locale(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.split('@').next().unwrap_or(value);
    let value = value.split('.').next().unwrap_or(value);
    if value.is_empty() || value == "C" || value == "POSIX" {
        return None;
    }
    Some(value.replace('_', "-"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "f-terminal-font-cache-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn write(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.path.join(name);
            fs::write(&path, contents).expect("write temp font");
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn sample_index(files: &[PathBuf]) -> FontIndex {
        let mut faces = Vec::new();
        let mut dirs = BTreeSet::new();

        for (index, path) in files.iter().enumerate() {
            let metadata = fs::metadata(path).expect("font metadata");
            dirs.insert(path.parent().expect("font parent").to_path_buf());
            faces.push(CachedFace {
                path: path.clone(),
                index: index as u32,
                families: vec!["Test Sans".to_owned(), "Test Sans Localized".to_owned()],
                post_script_name: format!("TestSans-{index}"),
                style: CachedStyle::Normal,
                weight: 400,
                stretch: CachedStretch::Normal,
                monospaced: true,
                file_size: metadata.len(),
                file_mtime_ns: file_mtime_ns(&metadata).expect("font mtime"),
            });
        }

        let dirs = dirs
            .into_iter()
            .map(|path| {
                let metadata = fs::metadata(&path).expect("dir metadata");
                CachedDir {
                    mtime_ns: file_mtime_ns(&metadata).expect("dir mtime"),
                    path,
                }
            })
            .collect();

        FontIndex {
            version: CACHE_VERSION,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: current_platform(),
            built_at_secs: unix_secs(SystemTime::now()).expect("clock after epoch"),
            faces,
            dirs,
            generic: GenericFamilies::default(),
        }
    }

    #[test]
    fn index_serialization_roundtrip() {
        let temp = TempDir::new();
        let first = temp.write("first.ttf", b"not really a font");
        let second = temp.write("second.ttf", b"another fake font");
        let index = sample_index(&[first, second]);

        let bytes = serde_json::to_vec(&index).expect("serialize index");
        let restored: FontIndex = serde_json::from_slice(&bytes).expect("deserialize index");

        assert_eq!(restored, index);
        assert_eq!(restored.faces[0].families[0], "Test Sans");
        assert_eq!(
            restored.faces[1].post_script_name,
            index.faces[1].post_script_name
        );
    }

    #[test]
    fn fresh_index_validates() {
        let temp = TempDir::new();
        let font = temp.write("fresh.ttf", b"fake font");
        let index = sample_index(&[font]);

        assert_eq!(index.validate(SystemTime::now()), Ok(()));
    }

    #[test]
    fn generic_families_are_preserved() {
        let temp = TempDir::new();
        let font = temp.write("generic.ttf", b"fake font");
        let mut index = sample_index(&[font]);
        index.generic.monospace = "Test Mono".to_owned();
        index.generic.sans_serif = "Test Sans UI".to_owned();

        let database = index.into_database();
        assert_eq!(
            database.family_name(&fontdb::Family::Monospace),
            "Test Mono"
        );
        assert_eq!(
            database.family_name(&fontdb::Family::SansSerif),
            "Test Sans UI"
        );
    }

    #[test]
    fn extreme_build_time_is_rejected() {
        let temp = TempDir::new();
        let font = temp.write("future.ttf", b"fake font");
        let mut index = sample_index(&[font]);
        index.built_at_secs = u64::MAX;

        assert!(index.validate(SystemTime::now()).is_err());
    }

    #[test]
    fn missing_font_file_invalidates() {
        let temp = TempDir::new();
        let font = temp.write("missing.ttf", b"fake font");
        let index = sample_index(std::slice::from_ref(&font));

        fs::remove_file(&font).expect("remove temp font");

        assert!(index.validate(SystemTime::now()).is_err());
    }

    #[test]
    fn stale_font_file_invalidates() {
        let temp = TempDir::new();
        let font = temp.write("stale.ttf", b"short");
        let index = sample_index(std::slice::from_ref(&font));

        fs::write(&font, b"a much longer replacement").expect("rewrite temp font");

        assert!(index.validate(SystemTime::now()).is_err());
    }

    #[test]
    fn changed_directory_invalidates() {
        let temp = TempDir::new();
        let font = temp.write("dir.ttf", b"fake font");
        let mut index = sample_index(&[font]);

        index.dirs[0].mtime_ns = index.dirs[0].mtime_ns.wrapping_add(1);

        assert!(index.validate(SystemTime::now()).is_err());
    }

    #[test]
    fn expired_index_invalidates() {
        let temp = TempDir::new();
        let font = temp.write("old.ttf", b"fake font");
        let mut index = sample_index(&[font]);

        index.built_at_secs = unix_secs(SystemTime::now())
            .expect("clock after epoch")
            .saturating_sub(MAX_AGE.as_secs() + 1);

        assert!(index.validate(SystemTime::now()).is_err());
    }

    #[test]
    fn written_cache_roundtrips_and_is_private() {
        let temp = TempDir::new();
        let font = temp.write("font.ttf", b"fake font");
        let index = sample_index(&[font]);
        let path = temp.path.join(CACHE_FILE_NAME);
        let bytes = serde_json::to_vec(&index).expect("serialize index");

        write_index(&path, &bytes).expect("write index");
        assert_eq!(read_index(&path), Some(index.clone()));

        // Overwriting goes through the same temp-file-and-rename path.
        write_index(&path, &bytes).expect("overwrite index");
        assert_eq!(read_index(&path), Some(index));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path)
                .expect("cache file metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_cache_is_rejected() {
        let temp = TempDir::new();
        let path = temp.write(CACHE_FILE_NAME, b"{ this is not a font index");

        assert!(read_index(&path).is_none());
    }

    #[test]
    fn oversized_cache_is_rejected() {
        let temp = TempDir::new();
        let path = temp.path.join(CACHE_FILE_NAME);
        let file = fs::File::create(&path).expect("create cache file");
        file.set_len(MAX_CACHE_BYTES + 1)
            .expect("extend cache file");
        drop(file);

        assert!(read_index(&path).is_none());
    }

    #[test]
    fn database_reconstruction_preserves_faces_and_generic_families() {
        let temp = TempDir::new();
        let first = temp.write("one.ttf", b"fake font one");
        let second = temp.write("two.ttf", b"fake font two");
        let index = sample_index(&[first, second]);

        let database = index.into_database();

        assert_eq!(database.len(), 2);
        assert_eq!(
            database.family_name(&fontdb::Family::Monospace),
            "Noto Sans Mono"
        );
        assert_eq!(
            database.family_name(&fontdb::Family::SansSerif),
            "Open Sans"
        );
        assert_eq!(database.family_name(&fontdb::Family::Serif), "DejaVu Serif");
        assert!(database.faces().all(|face| {
            face.families
                .iter()
                .all(|(_, language)| *language == fontdb::Language::English_UnitedStates)
        }));
    }

    #[test]
    fn locales_are_normalized() {
        assert_eq!(parse_locale("en_US.UTF-8"), Some("en-US".to_owned()));
        assert_eq!(parse_locale("de_DE.utf8@euro"), Some("de-DE".to_owned()));
        assert_eq!(parse_locale("C"), None);
        assert_eq!(parse_locale("C.UTF-8"), None);
        assert_eq!(parse_locale("POSIX"), None);
        assert_eq!(parse_locale(""), None);
    }
}
