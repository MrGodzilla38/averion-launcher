use std::{
    fs::{self, Metadata},
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

pub(crate) struct GameDirectory {
    root: PathBuf,
}

impl GameDirectory {
    pub(crate) fn discover() -> io::Result<Self> {
        #[cfg(target_os = "linux")]
        let base = std::env::home_dir();
        #[cfg(target_os = "windows")]
        let base = std::env::var_os("APPDATA").map(PathBuf::from);
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        let base = None;

        Self::from_user_base(base)
    }

    fn from_user_base(base: Option<PathBuf>) -> io::Result<Self> {
        let base = base
            .filter(|path| path.is_absolute())
            .ok_or(ErrorKind::NotFound)?;
        Ok(Self {
            root: base.join(".averion"),
        })
    }

    pub(crate) fn prepare(&self) -> io::Result<()> {
        ensure_directory(&self.root)?;
        for name in [
            "versions",
            "libraries",
            "assets",
            "java",
            "profiles",
            "config",
            "cache",
            "logs",
        ] {
            ensure_directory(&self.root.join(name))?;
        }
        ensure_directory(&self.root.join("assets/indexes"))?;
        ensure_directory(&self.metadata_cache())
    }

    pub(crate) fn metadata_cache(&self) -> PathBuf {
        self.root.join("cache/mojang")
    }

    pub(crate) fn version_directory(&self, version: &str) -> io::Result<PathBuf> {
        let path = self.root.join("versions").join(version);
        ensure_directory(&path)?;
        Ok(path)
    }

    pub(crate) fn asset_index(&self, id: &str) -> PathBuf {
        self.root.join("assets/indexes").join(format!("{id}.json"))
    }

    pub(crate) fn libraries(&self) -> PathBuf {
        self.root.join("libraries")
    }

    pub(crate) fn asset_objects(&self) -> io::Result<PathBuf> {
        checked_subdirectory(&self.root, Path::new("assets/objects"))
    }

    pub(crate) fn java(&self) -> PathBuf {
        self.root.join("java")
    }

    #[cfg(test)]
    pub(crate) fn for_test(path: &Path) -> Self {
        Self::from_user_base(Some(path.to_owned())).unwrap()
    }
}

pub(crate) fn checked_subdirectory(base: &Path, relative: &Path) -> io::Result<PathBuf> {
    use std::path::Component;

    let mut path = base.to_owned();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return Err(ErrorKind::InvalidInput.into());
        };
        path.push(name);
        ensure_directory(&path)?;
    }
    Ok(path)
}

pub(crate) fn ensure_directory(path: &Path) -> io::Result<()> {
    if let Err(error) = fs::create_dir(path)
        && error.kind() != ErrorKind::AlreadyExists
    {
        return Err(error);
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() && !is_link(&metadata) {
        Ok(())
    } else {
        Err(ErrorKind::InvalidInput.into())
    }
}

pub(crate) fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // FILE_ATTRIBUTE_REPARSE_POINT also covers Windows junctions.
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_an_absolute_root_and_rejects_missing_or_relative_paths() {
        let base = tempfile::tempdir().unwrap();
        let game = GameDirectory::from_user_base(Some(base.path().to_owned())).unwrap();
        assert_eq!(game.root, base.path().join(".averion"));
        assert!(!game.root.exists());
        assert!(GameDirectory::from_user_base(None).is_err());
        assert!(GameDirectory::from_user_base(Some("relative".into())).is_err());
        assert!(GameDirectory::from_user_base(Some("".into())).is_err());
    }

    #[test]
    fn creates_the_layout_idempotently_and_preserves_existing_files() {
        let base = tempfile::tempdir().unwrap();
        let game = GameDirectory::for_test(base.path());
        game.prepare().unwrap();
        let user_file = game.root.join("profiles/survival.json");
        fs::write(&user_file, "user data").unwrap();
        game.prepare().unwrap();
        assert_eq!(fs::read_to_string(user_file).unwrap(), "user data");
        for name in [
            "versions",
            "libraries",
            "assets/indexes",
            "java",
            "profiles",
            "config",
            "cache/mojang",
            "logs",
        ] {
            assert!(game.root.join(name).is_dir(), "{name}");
        }
    }

    #[test]
    fn preserves_an_existing_file_that_conflicts_with_a_directory() {
        let base = tempfile::tempdir().unwrap();
        let game = GameDirectory::for_test(base.path());
        fs::create_dir(&game.root).unwrap();
        fs::write(game.root.join("versions"), "user file").unwrap();
        assert_eq!(game.prepare().unwrap_err().kind(), ErrorKind::InvalidInput);
        assert_eq!(
            fs::read_to_string(game.root.join("versions")).unwrap(),
            "user file"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_managed_directories() {
        let base = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let game = GameDirectory::for_test(base.path());
        fs::create_dir(&game.root).unwrap();
        std::os::unix::fs::symlink(outside.path(), game.root.join("versions")).unwrap();
        assert_eq!(game.prepare().unwrap_err().kind(), ErrorKind::InvalidInput);
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    }
}
