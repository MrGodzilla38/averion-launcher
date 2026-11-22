use super::*;
use crate::download::{self, Failure, Stage};
use futures::{executor::block_on, future};
use serde_json::{Value, json};
use std::{cell::Cell, fs, path::Path, time::SystemTime};

const SELECTED: &str = "1.20";
const HASH: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
const MANIFEST_FILE: &str = "version-manifest-v2.cache";

fn version_json() -> Value {
    json!({
        "id": SELECTED,
        "mainClass": "net.minecraft.client.main.Main",
        "minecraftArguments": "--username ${auth_player_name}",
        "type": "release",
        "javaVersion": {"component":"java-runtime-gamma","majorVersion":17},
        "downloads": {"client": {
            "sha1": HASH, "size": 3, "url": "https://piston-data.mojang.com/client.jar"
        }},
        "libraries": [{
            "name": "com.example:fixture:1",
            "downloads": {"artifact": {
                "sha1": HASH, "size": 3, "path": "com/example/fixture/1/fixture-1.jar",
                "url": "https://libraries.minecraft.net/com/example/fixture/1/fixture-1.jar"
            }}
        }],
        "assetIndex": {
            "id": "5", "sha1": HASH, "size": 3, "url": "https://piston-meta.mojang.com/index.json"
        },
    })
}

fn entry(id: &str, kind: &str, hash: &str) -> Value {
    json!({
        "id": id, "type": kind, "sha1": hash, "url": "https://piston-meta.mojang.com/version.json"
    })
}

fn digest(bytes: &[u8]) -> [u8; 20] {
    Sha1::digest(bytes).into()
}

#[test]
fn manifest_uses_the_exact_selected_stable_release() {
    let entries = json!({"versions": [
        entry("1.20.1", "release", HASH),
        entry("1.21", "snapshot", HASH),
        entry(SELECTED, "release", HASH),
    ]});
    let manifest = Manifest::parse(&entries.to_string().into_bytes()).unwrap();
    assert_eq!(manifest.release(SELECTED).unwrap().id, SELECTED);
    assert!(matches!(
        manifest.release("1.21"),
        Err(Error::MissingRelease)
    ));
    assert!(matches!(
        manifest.release("1.18"),
        Err(Error::MissingRelease)
    ));

    let duplicate =
        json!({"versions": [entry(SELECTED, "release", HASH), entry(SELECTED, "snapshot", HASH)]});
    let manifest = Manifest::parse(duplicate.to_string().as_bytes()).unwrap();
    assert!(matches!(
        manifest.release(SELECTED),
        Err(Error::Invalid("duplicate version ID"))
    ));
    assert!(Manifest::parse(br#"{"versions":[]}"#).is_err());
    assert!(Manifest::parse(br#"{"versions":[{"id":"1.20"}]}"#).is_err());
}

#[test]
fn rejects_untrusted_urls_and_unsafe_ids() {
    for url in [
        "http://piston-meta.mojang.com/version.json",
        "https://piston-meta.mojang.com.evil.invalid/version.json",
        "https://piston-meta.mojang.com@evil.invalid/version.json",
        "https://user:secret@piston-meta.mojang.com/version.json",
        "https://piston-meta.mojang.com:8443/version.json",
        "https://piston-meta.mojang.com/version.json?foo=bar",
        "https://piston-meta.mojang.com/version.json#fragment",
    ] {
        assert!(trusted_url(url, METADATA_HOSTS).is_err(), "{url}");
    }
    assert!(trusted_url(MANIFEST_URL, METADATA_HOSTS).is_ok());
    assert!(trusted_url(MANIFEST_URL, CLIENT_HOSTS).is_err());
    for id in [
        "", "../1.20", "1..20", "5\\0", "5:", "CON", "5.", "/5", "５",
    ] {
        assert!(validate_id(id).is_err(), "{id}");
    }
}

#[test]
fn verifies_metadata_digest_and_validates_required_artifact_fields() {
    let valid = version_json();
    let original = valid.to_string();
    assert!(matches!(
        Version::parse(original.as_bytes(), SELECTED, [0; 20]),
        Err(Error::Invalid("version metadata checksum"))
    ));
    assert!(Version::parse(original.as_bytes(), SELECTED, digest(original.as_bytes())).is_ok());

    for (pointer, replacement) in [
        ("/id", json!("1.20.1")),
        ("/type", json!("snapshot")),
        ("/javaVersion/majorVersion", json!(0)),
        ("/javaVersion/component", json!("../escape")),
        ("/downloads/client/sha1", json!("incorrect")),
        ("/downloads/client/size", json!(0)),
        ("/downloads/client/size", Value::Null),
        (
            "/downloads/client/url",
            json!("https://other.invalid/client.jar"),
        ),
        ("/assetIndex/id", json!("../escape")),
        (
            "/assetIndex/url",
            json!("http://piston-meta.mojang.com/index.json"),
        ),
    ] {
        let mut document = valid.clone();
        *document.pointer_mut(pointer).unwrap() = replacement;
        let bytes = document.to_string();
        assert!(
            Version::parse(bytes.as_bytes(), SELECTED, digest(bytes.as_bytes())).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn cache_reads_fresh_entries_and_refreshes_stale_or_invalid_entries_safely() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        let calls = Cell::new(0);
        let calls = &calls;
        let read = |bytes: &[u8]| match bytes {
            b"fresh" => Ok(1),
            b"updated" => Ok(2),
            _ => Err(Error::Invalid("fixture")),
        };
        let fetch = |data: &'static [u8]| async move {
            calls.set(calls.get() + 1);
            Ok(data.to_vec())
        };
        assert_eq!(
            cache::load(&path, Some(MANIFEST_AGE), 32, read, fetch(b"fresh"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(
            cache::load(&path, Some(MANIFEST_AGE), 32, read, fetch(b"updated"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(calls.get(), 1);

        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - MANIFEST_AGE * 2)
            .unwrap();
        let bad = cache::load(&path, Some(MANIFEST_AGE), 32, read, fetch(b"invalid")).await;
        assert!(matches!(bad, Err(Error::Invalid(_))));
        assert_eq!(cache::read(&path, None, 32).unwrap().unwrap(), b"fresh");

        let failure = cache::load(
            &path,
            Some(MANIFEST_AGE),
            32,
            read,
            future::ready(Err(http::Error::Timeout)),
        )
        .await;
        assert_eq!(failure, Err(Error::Https(http::Error::Timeout)));
        assert_eq!(cache::read(&path, None, 32).unwrap().unwrap(), b"fresh");
        assert_eq!(
            cache::load(&path, Some(MANIFEST_AGE), 32, read, fetch(b"updated"))
                .await
                .unwrap(),
            2
        );

        write_cache(&path, b"corrupt");
        assert_eq!(
            cache::load(&path, None, 32, read, fetch(b"fresh"))
                .await
                .unwrap(),
            1
        );
        assert_eq!(cache::read(&path, None, 32).unwrap().unwrap(), b"fresh");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(cache::read(&path, None, 3).unwrap().is_none());
        let too_large = cache::load(&path, None, 3, read, fetch(b"updated")).await;
        assert_eq!(too_large, Err(Error::Https(http::Error::TooLarge)));
        assert_eq!(cache::read(&path, None, 32).unwrap().unwrap(), b"fresh");
    });
}

#[test]
fn cache_does_not_overwrite_an_untagged_file_at_a_cache_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(MANIFEST_FILE);
    fs::write(&path, b"existing user data").unwrap();
    let result = block_on(cache::load(&path, None, 32, |_| Ok(()), async {
        panic!("a cache path conflict must be reported before fetching");
    }));
    assert_eq!(result, Err(Error::Cache(io::ErrorKind::AlreadyExists)));
    assert_eq!(fs::read(path).unwrap(), b"existing user data");
}

#[cfg(unix)]
#[test]
fn cache_rejects_symlinks_and_preserves_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("user.json");
    let link = dir.path().join(MANIFEST_FILE);
    fs::write(&target, "user data").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(cache::read(&link, None, MANIFEST_LIMIT).is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "user data");
}

fn write_cache(path: &Path, payload: &[u8]) {
    fs::write(path, [cache::CACHE_TAG, payload].concat()).unwrap();
}

fn seed_cache(directory: &GameDirectory) {
    use std::fmt::Write;

    directory.prepare().unwrap();
    let metadata = version_json().to_string();
    let mut sha1 = String::with_capacity(40);
    for byte in digest(metadata.as_bytes()) {
        write!(sha1, "{byte:02x}").unwrap();
    }
    let cache = directory.metadata_cache();
    let manifest =
        json!({"versions": [entry("1.20.1", "release", HASH), entry(SELECTED, "release", &sha1)]});
    write_cache(&cache.join(MANIFEST_FILE), manifest.to_string().as_bytes());
    write_cache(
        &cache.join(format!("{SELECTED}-{sha1}.cache")),
        metadata.as_bytes(),
    );
}

#[test]
fn cached_metadata_builds_a_verified_plan_for_only_the_selected_version() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    seed_cache(&directory);
    // No Tokio runtime is available here. A cache miss would fail the test.
    let (_sender, cancel) = download::cancellation();
    let plan = block_on(Mojang::default().resolve_in(SELECTED, &directory, &cancel)).unwrap();
    assert_eq!(plan.files.len(), 2);
    assert_eq!(plan.java_version.major, 17);
    assert_eq!(plan.java_version.component, "java-runtime-gamma");
    for artifact in plan.files.iter().chain(std::iter::once(&plan.index)) {
        assert_eq!(artifact.expected_size, Some(3));
        assert_eq!(artifact.checksum, Some(Checksum::Sha1(digest(b"abc"))));
        assert_eq!(artifact.existing, ExistingFile::Preserve);
        assert!(!artifact.destination.exists());
    }
    let root = base.path().join(".averion");
    assert_eq!(
        plan.files[0].destination,
        root.join("versions/1.20/1.20.jar")
    );
    assert_eq!(
        plan.files[1].destination,
        root.join("libraries/com/example/fixture/1/fixture-1.jar")
    );
    assert_eq!(plan.index.destination, root.join("assets/indexes/5.json"));
    assert_eq!(fs::read_dir(root.join("versions")).unwrap().count(), 1);
}

#[test]
#[ignore = "downloads and verifies the official 1.16 game files in a temporary directory"]
fn official_https_complete_installation() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        let mojang = Mojang::default();
        let (_sender, cancel) = download::cancellation();
        let report = |_: Progress| future::ready(()).boxed();
        mojang
            .install_in("1.16", &directory, false, &cancel, &report)
            .await
            .unwrap();
        let version = directory.version_directory("1.16").unwrap();
        assert!(version.join("1.16.jar").is_file());
        assert!(
            fs::read_dir(version.join("natives"))
                .unwrap()
                .next()
                .is_some()
        );

        let network_read = Arc::new(AtomicBool::new(false));
        let report = {
            let network_read = network_read.clone();
            move |progress: Progress| {
                if matches!(progress.stage, Stage::Receiving { .. }) {
                    network_read.store(true, Ordering::Relaxed);
                }
                future::ready(()).boxed()
            }
        };
        mojang
            .install_in("1.16", &directory, false, &cancel, &report)
            .await
            .unwrap();
        assert!(!network_read.load(Ordering::Relaxed));
    });
}

#[test]
fn mojang_plan_preserves_conflicting_user_files() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    seed_cache(&directory);
    let (_sender, cancel) = download::cancellation();
    let plan = block_on(Mojang::default().resolve_in(SELECTED, &directory, &cancel)).unwrap();
    let existing: &Path = &plan.files[0].destination;
    fs::write(existing, b"old").unwrap();
    let error = block_on(download::run(&plan.files[..1], cancel, |_| {
        future::ready(())
    }))
    .unwrap_err();
    assert_eq!(error.failure, Failure::ExistingFile);
    assert_eq!(fs::read(existing).unwrap(), b"old");
    assert!(!plan.files[1].destination.exists());
}

fn cached_file(path: PathBuf, bytes: &[u8]) -> Download {
    fs::write(&path, bytes).unwrap();
    Download {
        name: "Cached fixture".into(),
        destination: path,
        expected_size: Some(bytes.len() as u64),
        checksum: Some(Checksum::Sha1(digest(bytes))),
        existing: ExistingFile::Preserve,
        open: Box::new(|| panic!("verified fixtures must be reused")),
    }
}

#[test]
fn an_invalid_native_archive_prevents_completion_after_verified_downloads() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    directory.prepare().unwrap();
    let version_dir = directory.version_directory(SELECTED).unwrap();
    let index = json!({"objects": {"test/object": {"hash":HASH,"size":3}}});
    let index = cached_file(directory.asset_index("5"), index.to_string().as_bytes());
    let client = cached_file(version_dir.join("1.20.jar"), b"abc");
    let native = cached_file(
        directory.libraries().join("native.jar"),
        b"verified but invalid ZIP",
    );
    let native_archive = natives::NativeArchive {
        name: native.name.clone(),
        path: native.destination.clone(),
        sha1: digest(b"verified but invalid ZIP"),
        size: native.expected_size.unwrap(),
        excludes: Vec::new(),
    };
    let asset_parent = directory.asset_objects().unwrap().join("a9");
    fs::create_dir(&asset_parent).unwrap();
    fs::write(asset_parent.join(HASH), b"abc").unwrap();
    let plan = InstallationPlan {
        index,
        files: vec![client, native],
        metadata: Vec::new(),
        metadata_sha1: [0; 20],
        logging: None,
        repair: false,
        natives: vec![native_archive],
        version_dir,
        platform: Platform::current().unwrap(),
        java_version: java::Requirement {
            component: "java-runtime-gamma".into(),
            major: 17,
        },
    };
    let (_sender, cancel) = download::cancellation();
    let report = |_: Progress| future::ready(()).boxed();
    assert!(matches!(
        block_on(Mojang::default().execute(SELECTED, &directory, plan, &cancel, &report)),
        Err(Error::Native("invalid ZIP archive"))
    ));
}

#[test]
fn prepares_legacy_game_and_detects_same_size_classpath_damage() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    directory.prepare().unwrap();
    let version_dir = directory.version_directory(SELECTED).unwrap();
    fs::create_dir_all(version_dir.join("natives/launch")).unwrap();
    let client = version_dir.join("1.20.jar");
    fs::write(&client, b"abc").unwrap();
    let library = directory
        .libraries()
        .join("com/example/fixture/1/fixture-1.jar");
    fs::create_dir_all(library.parent().unwrap()).unwrap();
    fs::write(&library, b"abc").unwrap();
    let assets = json!({"objects": {"fixture/asset": {"hash":HASH,"size":3}}}).to_string();
    fs::write(directory.asset_index("5"), &assets).unwrap();
    let object = directory.asset_objects().unwrap().join("a9").join(HASH);
    fs::create_dir_all(object.parent().unwrap()).unwrap();
    fs::write(object, b"abc").unwrap();

    let mut metadata = version_json();
    metadata["assetIndex"]["size"] = json!(assets.len());
    metadata["assetIndex"]["sha1"] = json!(hex_sha1(&digest(assets.as_bytes())));
    let metadata = serde_json::to_vec(&metadata).unwrap();
    let record = Record {
        schema: record::SCHEMA,
        version: SELECTED.into(),
        metadata_sha1: hex_sha1(&digest(&metadata)),
        runtime: Runtime {
            executable: base.path().join("java"),
            major: 17,
        },
        natives: vec![],
        log_config: None,
    };
    record::write(&version_dir, SELECTED, &metadata, &record).unwrap();
    let prepared = prepare(&directory, SELECTED).unwrap();
    assert!(prepared.arguments.game.is_none());
    assert_eq!(
        prepared.legacy.as_deref(),
        Some("--username ${auth_player_name}")
    );
    assert_eq!(prepared.classpath, [client, library.clone()]);
    let (_keep_active, cancel) = download::cancellation();
    verify(&prepared, &cancel).unwrap();
    fs::write(&library, b"bad").unwrap();
    assert!(matches!(verify(&prepared, &cancel), Err(Error::Damaged(_))));
}
