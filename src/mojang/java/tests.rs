use super::*;
use futures::{FutureExt, executor::block_on, future};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn requirement(major: u16, component: &str) -> Requirement {
    Requirement {
        major,
        component: component.into(),
    }
}

#[test]
fn selects_the_official_component_for_the_requested_platform_and_major() {
    let hash = "a9993e364706816aba3e25717850c26c9cd0d89d";
    let release = |progress, version, suffix| {
        json!({
            "availability": {"progress": progress},
            "manifest": {"sha1": hash, "size": 200, "url": format!("https://piston-meta.mojang.com/{suffix}")},
            "version": {"name": version},
        })
    };
    let catalog = json!({
        "linux": {"java-runtime-gamma": [
            release(50, "17.0.16", "linux-unavailable"),
            release(100, "21.0.7", "linux-major-mismatch"),
            release(100, "17.0.15", "linux"),
        ]},
        "windows-x64": {"java-runtime-gamma":[release(100, "17.0.15", "windows")]},
    })
    .to_string();
    let requirement = requirement(17, "java-runtime-gamma");
    for (platform, path) in [("linux", "/linux"), ("windows-x64", "/windows")] {
        assert_eq!(
            select_catalog(catalog.as_bytes(), platform, &requirement)
                .unwrap()
                .url
                .path(),
            path
        );
    }
    assert!(select_catalog(catalog.as_bytes(), "windows-arm64", &requirement).is_err());
}

#[test]
fn managed_java_versions_have_independent_platform_directories() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    directory.prepare().unwrap();
    let java_17 = managed_root(&directory, 17, "linux").unwrap();
    let java_21 = managed_root(&directory, 21, "linux").unwrap();
    let java_21_windows = managed_root(&directory, 21, "windows-x64").unwrap();
    assert_eq!(java_17, directory.java().join("17/linux"));
    assert!(java_21.is_dir());
    assert!(java_21_windows.is_dir());
    assert_ne!(java_17, java_21);
    assert_ne!(java_21, java_21_windows);
}

#[test]
fn cancellation_stops_runtime_resolution_before_network_or_processes() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    let (sender, cancel) = crate::download::cancellation();
    sender.send(()).unwrap();
    let report = |_: Progress| future::ready(()).boxed();
    let result = block_on(resolve(
        &requirement(17, "java-runtime-gamma"),
        &directory,
        &Https::default(),
        &cancel,
        &report,
    ));
    assert!(matches!(result, Err(Error::Download(error)) if error.failure == Failure::Cancelled));
    assert!(!directory.java().exists());
}

#[test]
#[ignore = "installs and reuses the official Mojang Java 21 runtime over HTTPS"]
fn official_managed_java_installation() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        directory.prepare().unwrap();
        let https = Https::default();
        let (_sender, cancel) = crate::download::cancellation();
        let requirement = requirement(21, "java-runtime-delta");
        let progress = Arc::new(Mutex::new((false, false, false)));
        let report = {
            let progress = progress.clone();
            move |progress_event: Progress| {
                let mut seen = progress.lock().unwrap();
                seen.0 |= matches!(progress_event.stage, Stage::Resolving);
                seen.1 |= matches!(progress_event.stage, Stage::Receiving { .. });
                seen.2 |= matches!(progress_event.stage, Stage::Extracting);
                future::ready(()).boxed()
            }
        };
        let runtime = managed(&requirement, &directory, &https, &cancel, &report)
            .await
            .unwrap();
        assert_eq!(runtime.major, 21);
        assert!(runtime.executable.starts_with(directory.java().join("21")));
        assert!(runtime.executable.is_file());
        assert_eq!(*progress.lock().unwrap(), (true, true, true));
        *progress.lock().unwrap() = (false, false, false);
        let reused = managed(&requirement, &directory, &https, &cancel, &report)
            .await
            .unwrap();
        assert_eq!(reused, runtime);
        assert_eq!(*progress.lock().unwrap(), (false, false, false));
    });
}

#[test]
#[ignore = "requires a compatible system Java 17 on the host"]
fn detects_system_java_without_provisioning() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let base = tempfile::tempdir().unwrap();
        let directory = GameDirectory::for_test(base.path());
        let (_sender, cancel) = crate::download::cancellation();
        let report = |progress: Progress| {
            assert_eq!(progress.stage, Stage::Checking);
            future::ready(()).boxed()
        };
        let runtime = resolve(
            &requirement(17, "java-runtime-gamma"),
            &directory,
            &Https::default(),
            &cancel,
            &report,
        )
        .await
        .unwrap();
        assert_eq!(runtime.major, 17);
        assert!(!runtime.executable.starts_with(directory.java()));
        assert!(!directory.java().exists());
    });
}

#[test]
#[ignore = "validates official Linux and Windows Java manifests over HTTPS"]
fn supports_the_published_runtime_manifests() {
    use futures::StreamExt;

    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let https = Https::default();
        let catalog = https
            .metadata(trusted_url(CATALOG, METADATA_HOSTS).unwrap(), CATALOG_LIMIT)
            .await
            .unwrap();
        let components = [
            (8, "jre-legacy"),
            (16, "java-runtime-alpha"),
            (17, "java-runtime-beta"),
            (17, "java-runtime-gamma"),
            (21, "java-runtime-delta"),
            (25, "java-runtime-epsilon"),
        ];
        let requests = ["linux", "windows-x64"].into_iter().flat_map(|platform| {
            components
                .into_iter()
                .map(move |(major, component)| (platform, major, component))
        });
        let mut checks = futures::stream::iter(requests)
            .map(|(platform, major, component)| {
                let manifest =
                    select_catalog(&catalog, platform, &requirement(major, component)).unwrap();
                let https = &https;
                async move {
                    let bytes = https.metadata(manifest.url, PACKAGE_LIMIT).await.unwrap();
                    assert!(
                        package::Package::parse(
                            &bytes,
                            &manifest.sha1,
                            Some(manifest.size),
                            platform
                        )
                        .is_ok(),
                        "{platform} {component}"
                    );
                }
            })
            .buffer_unordered(4);
        while checks.next().await.is_some() {}
    });
}
