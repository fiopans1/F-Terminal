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
