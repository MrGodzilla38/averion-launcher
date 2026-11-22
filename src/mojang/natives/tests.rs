use super::*;
use std::io::Cursor;
use zip::{ZipWriter, write::SimpleFileOptions};

const LINUX: Platform = Platform { os: "linux" };

fn archive(base: &Path, files: &[(&str, &[u8])], symlink: Option<&str>) -> NativeArchive {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for &(name, bytes) in files {
        zip.start_file(name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    if let Some(name) = symlink {
        zip.add_symlink(name, "../user-file", options).unwrap();
    }
    let bytes = zip.finish().unwrap().into_inner();
    let path = base.join("native.jar");
    fs::write(&path, &bytes).unwrap();
    NativeArchive {
        name: "Fixture natives".into(),
        path,
        sha1: Sha1::digest(&bytes).into(),
        size: bytes.len() as u64,
        excludes: vec!["ignore/".into()],
    }
}

#[test]
fn extracts_only_native_entries_and_reuses_identical_existing_outputs() {
    let base = tempfile::tempdir().unwrap();
    let native = archive(
        base.path(),
        &[
            ("linux/x64/libfixture.so", b"test"),
            ("linux/x64/libfixture.so.sha1", b"metadata"),
            ("META-INF/injected.so", b"metadata"),
            ("ignore/skip.so", b"rule exclusion"),
        ],
        None,
    );
    let (_sender, cancel) = crate::download::cancellation();
    extract(&native, base.path(), LINUX, &cancel).unwrap();
    let target = base.path().join("natives").join(hex_sha1(&native.sha1));
    let output = target.join("linux/x64/libfixture.so");
    assert_eq!(fs::read(&output).unwrap(), b"test");
    for skipped in ["linux/x64/libfixture.so.sha1", "META-INF", "ignore"] {
        assert!(!target.join(skipped).exists());
    }
    extract(&native, base.path(), LINUX, &cancel).unwrap();
    fs::write(&output, b"user").unwrap();
    assert_eq!(
        extract(&native, base.path(), LINUX, &cancel),
        Err(Error::Native("conflicting native file was preserved"))
    );
    assert_eq!(fs::read(&output).unwrap(), b"user");
    assert_eq!(fs::read_dir(target.join("linux/x64")).unwrap().count(), 1);
}

#[test]
fn refuses_traversal_symlinks_and_case_colliding_paths_before_extracting() {
    for (name, link) in [
        ("../outside.so", None),
        ("/root.so", None),
        ("C:/absolute.so", None),
        ("pkg/../traversal.so", None),
        ("pkg\\escape.so", None),
        ("PKG/LEGIT.SO", None),
        ("pkg/normal.so", Some("ignored/link.so")),
    ] {
        let base = tempfile::tempdir().unwrap();
        let native = archive(
            base.path(),
            &[("pkg/legit.so", b"safe"), (name, b"unsafe")],
            link,
        );
        let (_sender, cancel) = crate::download::cancellation();
        assert!(
            matches!(
                extract(&native, base.path(), LINUX, &cancel),
                Err(Error::Native(_))
            ),
            "{name}"
        );
        assert!(!base.path().join("natives").exists(), "{name}");
    }
}

#[test]
fn verifies_archive_digest_and_respects_cancellation_before_publication() {
    let base = tempfile::tempdir().unwrap();
    let mut native = archive(base.path(), &[("libfixture.so", b"test")], None);
    let (sender, cancel) = crate::download::cancellation();
    native.sha1 = [0; 20];
    assert_eq!(
        extract(&native, base.path(), LINUX, &cancel),
        Err(Error::Native("native archive checksum mismatch"))
    );
    sender.send(()).unwrap();
    assert!(
        matches!(extract(&native, base.path(), LINUX, &cancel), Err(Error::Download(error)) if error.failure == crate::download::Failure::Cancelled)
    );
    assert!(!base.path().join("natives").exists());
}

#[test]
fn assembles_nested_lwjgl_native_paths() {
    let base = tempfile::tempdir().unwrap();
    let version = base.path();
    let archive = "fixture-archive";
    let relative = PathBuf::from("linux/x64/org/lwjgl/liblwjgl.so");
    let original = version.join("natives").join(archive).join(&relative);
    fs::create_dir_all(original.parent().unwrap()).unwrap();
    fs::write(original, b"native fixture").unwrap();
    let (_sender, cancel) = crate::download::cancellation();
    let outputs = assemble(
        version,
        &[(archive.into(), vec![relative.clone()])],
        &cancel,
    )
    .unwrap();
    assert_eq!(outputs[0].0, PathBuf::from("natives/launch").join(relative));
    assert_eq!(
        fs::read(version.join(&outputs[0].0)).unwrap(),
        b"native fixture"
    );
}
