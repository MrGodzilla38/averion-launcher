use super::*;
use serde_json::{Value, json};

const LINUX: Platform = Platform { os: "linux" };
const WINDOWS: Platform = Platform { os: "windows" };
const HASH: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";

fn file(path: &str) -> Value {
    json!({"path": path, "url": format!("https://libraries.minecraft.net/{path}"), "size": 3, "sha1": HASH})
}

#[test]
fn matching_rules_are_applied_in_order_with_os_and_arch_gates() {
    let cases = [
        (json!([]), false),
        (json!([{"action":"disallow"}]), false),
        (
            json!([{"action":"allow"},{"action":"disallow","os":{"name":"windows"}}]),
            false,
        ),
        (
            json!([{"action":"disallow"},{"action":"allow","os":{"name":"windows","arch":"amd64"}}]),
            true,
        ),
        (
            json!([{"action":"allow","os":{"name":"windows","arch":"x86"}}]),
            false,
        ),
        (
            json!([{"action":"allow","features":{"is_demo_user":true}}]),
            false,
        ),
        (
            json!([{"action":"allow","features":{"is_demo_user":false}}]),
            true,
        ),
    ];
    assert!(allowed(None, WINDOWS).unwrap());
    for (rules, expected) in cases {
        let rules: Vec<Rule> = serde_json::from_value(rules).unwrap();
        assert_eq!(allowed(Some(&rules), WINDOWS).unwrap(), expected);
    }
    for os in [
        json!({"name":"windows","version":"^10"}),
        json!({"arch":"x86.*"}),
    ] {
        let rule = serde_json::from_value(json!([{"action":"allow","os": os}])).unwrap();
        let rules: Vec<Rule> = rule;
        assert!(allowed(Some(&rules), WINDOWS).is_err());
    }
}

#[test]
fn resolves_legacy_and_modern_natives_for_the_current_platform() {
    let fixtures = json!([
        {
            "name":"com.example:legacy:1",
            "downloads":{
                "artifact": file("com/example/legacy/1/legacy-1.jar"),
                "classifiers": {
                    "natives-windows-64":file("com/example/legacy/1/legacy-1-natives-windows-64.jar"),
                    "natives-linux":file("com/example/legacy/1/legacy-1-natives-linux.jar")
                }
            },
            "natives":{"linux":"natives-linux","windows":"natives-windows-${arch}"},
            "extract":{"exclude":["skip/"]}
        },
        {
            "name":"com.example:modern:1:natives-windows",
            "downloads":{"artifact": file("com/example/modern/1/modern-1-natives-windows.jar")},
            "rules":[{"action":"allow","os":{"name":"windows"}}]
        },
        {
            "name":"com.example:modern:1:natives-windows-arm64",
            "downloads":{"artifact": file("com/example/modern/1/modern-1-natives-windows-arm64.jar")},
            "rules":[{"action":"allow","os":{"name":"windows"}}]
        },
        {
            "name":"com.example:modern:1:natives-windows-x86",
            "downloads":{"artifact": file("com/example/modern/1/modern-1-natives-windows-x86.jar")},
            "rules":[{"action":"allow","os":{"name":"windows"}}]
        },
        {
            "name":"com.example:modern:1:natives-linux",
            "downloads":{"artifact": file("com/example/modern/1/modern-1-natives-linux.jar")},
            "rules":[{"action":"allow","os":{"name":"linux"}}]
        }
    ]);
    let base = tempfile::tempdir().unwrap();
    let (_sender, cancel) = crate::download::cancellation();
    for (platform, legacy, modern) in [
        (
            LINUX,
            "legacy-1-natives-linux.jar",
            "modern-1-natives-linux.jar",
        ),
        (
            WINDOWS,
            "legacy-1-natives-windows-64.jar",
            "modern-1-natives-windows.jar",
        ),
    ] {
        let resolved = resolve(
            serde_json::from_value(fixtures.clone()).unwrap(),
            platform,
            base.path(),
            &Https::default(),
            &cancel,
        )
        .unwrap();
        assert_eq!(resolved.downloads.len(), 3);
        assert_eq!(resolved.natives.len(), 2);
        assert_eq!(
            resolved.natives[0].path,
            base.path().join("com/example/legacy/1").join(legacy)
        );
        assert_eq!(resolved.natives[0].excludes, ["skip/"]);
        assert_eq!(
            resolved.natives[1].path,
            base.path().join("com/example/modern/1").join(modern)
        );
    }
}

#[test]
fn fails_closed_on_required_missing_or_unsafe_artifacts() {
    let base = tempfile::tempdir().unwrap();
    let (_sender, cancel) = crate::download::cancellation();
    for library in [
        json!({"name":"com.example:legacy:1", "natives":{"linux":"natives-linux"}}),
        json!({"name":"com.example:legacy:1","downloads":{"artifact": file("../outside.jar")}}),
        json!({"name":"com.example:legacy:1","downloads":{"artifact": file("a/NUL.jar")}}),
        json!({"name":"com.example:legacy:1","downloads":{"artifact":{"path":"x/a.jar","url":"https://untrusted.invalid/x/a.jar","size":1,"sha1":HASH}}}),
    ] {
        assert!(
            resolve(
                vec![serde_json::from_value(library).unwrap()],
                LINUX,
                base.path(),
                &Https::default(),
                &cancel
            )
            .is_err()
        );
    }
}
