use super::*;
use std::cell::RefCell;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const CLIENT: &str = "11111111-2222-3333-4444-555555555555";
const UUID: &str = "12345678-1234-4abc-abcd-123456789abc";
const OAUTH: &str = r#"{"access_token":"mock-ms-access","refresh_token":"mock-rotated-refresh","token_type":"Bearer","expires_in":3600}"#;
const XBOX: &str =
    r#"{"Token":"mock-xbox","DisplayClaims":{"xui":[{"uhs":"1234567890123456","xid":"4567"}]}}"#;
const XSTS: &str = r#"{"Token":"mock-xsts","DisplayClaims":{"xui":[{"uhs":"1234567890123456"}]}}"#;
const GAME: &str =
    r#"{"access_token":"mock-minecraft-access","token_type":"Bearer","expires_in":86400}"#;
const ENTITLEMENTS: &str = r#"{"items":[{"name":"product_minecraft"}]}"#;
const PROFILE: &str = r#"{"name":"ExamplePlayer","id":"1234567812344abcabcd123456789abc"}"#;

#[test]
fn sign_in_configuration_distinguishes_missing_and_invalid_ids() {
    use std::ffi::OsStr;

    assert_eq!(resolve_client_id(None, None), Err(Error::MissingClientId));
    assert_eq!(resolve_client_id(None, Some(CLIENT)).as_deref(), Ok(CLIENT));
    let runtime = "ABCDEFAB-1234-5678-9ABC-DEFABCDEFABC";
    assert_eq!(
        resolve_client_id(Some(OsStr::new(runtime)), Some(CLIENT)),
        Ok(runtime.to_ascii_lowercase())
    );
    for invalid in ["", "not-an-app-id", "11111111222233334444555555555555"] {
        assert_eq!(
            resolve_client_id(Some(OsStr::new(invalid)), Some(CLIENT)),
            Err(Error::InvalidClientId)
        );
    }
}

struct Step {
    start: &'static str,
    contains: &'static [&'static str],
    status: &'static str,
    body: &'static str,
}

impl Step {
    fn ok(start: &'static str, contains: &'static [&'static str], body: &'static str) -> Self {
        Self {
            start,
            contains,
            status: "200 OK",
            body,
        }
    }
}

fn game_steps() -> Vec<Step> {
    vec![
        Step::ok(
            "POST /xbox",
            &[
                "x-xbl-contract-version: 1",
                "d=mock-ms-access",
                "http://auth.xboxlive.com",
            ],
            XBOX,
        ),
        Step::ok(
            "POST /xsts",
            &["mock-xbox", "RETAIL", "rp://api.minecraftservices.com/"],
            XSTS,
        ),
        Step::ok(
            "POST /minecraft",
            &["XBL3.0 x=1234567890123456;mock-xsts"],
            GAME,
        ),
        Step::ok(
            "GET /entitlements",
            &["authorization: Bearer mock-minecraft-access"],
            ENTITLEMENTS,
        ),
        Step::ok(
            "GET /profile",
            &["authorization: Bearer mock-minecraft-access"],
            PROFILE,
        ),
    ]
}

async fn services(steps: Vec<Step>) -> (Services, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        for step in steps {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut bytes = [0; 4096];
            loop {
                let read = stream.read(&mut bytes).await.unwrap();
                assert!(read > 0 && request.len() + read <= 65536);
                request.extend_from_slice(&bytes[..read]);
                if let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let headers = std::str::from_utf8(&request[..end]).unwrap();
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let request = std::str::from_utf8(&request).unwrap();
            assert!(request.starts_with(step.start), "{request}");
            assert!(!request.contains("client_secret"));
            for expected in step.contains {
                assert!(
                    request.contains(expected),
                    "missing {expected} in {request}"
                );
            }
            stream.write_all(format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                step.status, step.body.len(), step.body,
            ).as_bytes()).await.unwrap();
        }
    });
    (
        Services {
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            endpoints: Endpoints {
                oauth: format!("{base}/token"),
                xbox: format!("{base}/xbox"),
                xsts: format!("{base}/xsts"),
                minecraft: format!("{base}/minecraft"),
                entitlements: format!("{base}/entitlements"),
                profile: format!("{base}/profile"),
            },
        },
        task,
    )
}

fn run(test: impl std::future::Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(test);
}

#[test]
fn exchanges_pkce_code_checks_ownership_and_builds_authenticated_launch_session() {
    run(async {
        let mut steps = vec![Step::ok(
            "POST /token",
            &[
                "client_id=11111111-2222-3333-4444-555555555555",
                "scope=XboxLive.signin+offline_access",
                "grant_type=authorization_code",
                "code=mock-code",
                "code_verifier=mock-verifier",
                "redirect_uri=http%3A%2F%2Flocalhost%3A8765%2F",
            ],
            OAUTH,
        )];
        steps.extend(game_steps());
        let (services, server) = services(steps).await;
        let tokens = services
            .exchange(
                CLIENT,
                &[
                    ("grant_type", "authorization_code"),
                    ("code", "mock-code"),
                    ("code_verifier", "mock-verifier"),
                    ("redirect_uri", "http://localhost:8765/"),
                ],
            )
            .await
            .unwrap();
        assert_eq!(
            tokens.refresh_token.unwrap().0.as_str(),
            "mock-rotated-refresh"
        );
        let mut progress = Vec::new();
        let verified = services
            .verify(&tokens.access_token, |stage| progress.push(stage))
            .await
            .unwrap();
        let session = verified.session(CLIENT).unwrap();
        assert_eq!(
            progress,
            [
                Stage::Xbox,
                Stage::Minecraft,
                Stage::Ownership,
                Stage::Profile
            ]
        );
        assert_eq!(session.player_name, "ExamplePlayer");
        assert_eq!(session.uuid, UUID);
        assert_eq!(session.user_type, "msa");
        assert_eq!(session.access_token.as_str(), "mock-minecraft-access");
        assert_eq!(session.client_id, CLIENT);
        assert_eq!(session.xuid, "4567");
        server.await.unwrap();
    });
}

#[test]
fn refreshes_session_and_requires_the_same_uuid() {
    run(async {
        for selected in [UUID, "99999999-9999-4999-a999-999999999999"] {
            let mut steps = vec![Step::ok(
                "POST /token",
                &["grant_type=refresh_token", "refresh_token=mock-old-refresh"],
                OAUTH,
            )];
            steps.extend(game_steps());
            let (services, server) = services(steps).await;
            match refresh_session(&services, CLIENT, selected, "mock-old-refresh").await {
                Ok((verified, rotated)) if selected == UUID => {
                    assert_eq!(verified.account.uuid, selected);
                    assert_eq!(rotated.unwrap().0.as_str(), "mock-rotated-refresh");
                }
                Err(Error::WrongProfile) if selected != UUID => {}
                _ => panic!("unexpected refresh result"),
            }
            server.await.unwrap();
        }
    });
}

#[test]
fn rejects_bedrock_only_entitlement_and_reports_missing_or_invalid_profile() {
    run(async {
        for (index, response, expected) in [
            (
                3,
                Step::ok(
                    "GET /entitlements",
                    &[],
                    r#"{"items":[{"name":"product_minecraft_bedrock"}]}"#,
                ),
                Error::NoOwnership,
            ),
            (
                4,
                Step {
                    start: "GET /profile",
                    contains: &[],
                    status: "404 Not Found",
                    body: "{}",
                },
                Error::NoProfile,
            ),
            (
                4,
                Step::ok(
                    "GET /profile",
                    &[],
                    r#"{"name":"ExamplePlayer","id":"malformed"}"#,
                ),
                Error::Response(Stage::Profile),
            ),
            (
                1,
                Step {
                    start: "POST /xsts",
                    contains: &[],
                    status: "401 Unauthorized",
                    body: r#"{"XErr":2148916238}"#,
                },
                Error::XboxAge,
            ),
            (
                2,
                Step {
                    start: "POST /minecraft",
                    contains: &[],
                    status: "403 Forbidden",
                    body: "{}",
                },
                Error::AppRegistration,
            ),
        ] {
            let mut steps = game_steps();
            steps.truncate(index);
            steps.push(response);
            let (services, server) = services(steps).await;
            let err = services
                .verify(&Secret(Zeroizing::new("mock-ms-access".into())), |_| {})
                .await
                .err()
                .unwrap();
            assert_eq!(err, expected);
            server.await.unwrap();
        }
    });
}

#[test]
fn expired_tokens_revoked_credentials_and_network_errors_fail_closed() {
    assert!(expiration(0, "Bearer", Stage::Minecraft, Instant::now()).is_err());
    assert!(expiration(3600, "different", Stage::Minecraft, Instant::now()).is_err());
    let expired = Verified {
        account: Account::microsoft("ExamplePlayer".into(), UUID).unwrap(),
        token: Secret(Zeroizing::new("mock-token".into())),
        expires_at: Instant::now(),
        xuid: String::new(),
    };
    assert!(matches!(
        expired.session(CLIENT),
        Err(Error::Response(Stage::Minecraft))
    ));
    run(async {
        let (services, server) = services(vec![Step {
            start: "POST /token",
            contains: &["grant_type=refresh_token"],
            status: "400 Bad Request",
            body: r#"{"error":"invalid_grant","error_description":"must not be in a diagnostic"}"#,
        }])
        .await;
        let result = refresh_session(&services, CLIENT, UUID, "mock-old-refresh").await;
        assert!(matches!(result, Err(Error::SignInAgain)));
        server.await.unwrap();
        assert!(matches!(
            refresh_session(&services, CLIENT, UUID, "mock-old-refresh").await,
            Err(Error::Network(Stage::Microsoft))
        ));
    });
}

#[derive(Default)]
struct MemoryVault {
    refresh: RefCell<Option<Zeroizing<String>>>,
    unavailable: bool,
}

impl Vault for MemoryVault {
    fn read(&self, _: &str) -> Result<Zeroizing<String>, Error> {
        self.refresh.borrow().clone().ok_or(Error::SignInAgain)
    }

    fn write(&self, _: &str, refresh: &str) -> Result<(), Error> {
        if self.unavailable {
            return Err(Error::VaultUnavailable);
        }
        *self.refresh.borrow_mut() = Some(Zeroizing::new(refresh.to_owned()));
        Ok(())
    }

    fn remove(&self, _: &str) -> Result<(), Error> {
        self.refresh.borrow_mut().take();
        Ok(())
    }
}

#[test]
fn saves_only_public_data_and_leaves_the_active_account_on_vault_failure() {
    let base = tempfile::tempdir().unwrap();
    let directory = GameDirectory::for_test(base.path());
    super::super::save_offline(&directory, "OfflinePlayer").unwrap();
    let account = Account::microsoft("ExamplePlayer".into(), UUID).unwrap();
    let unavailable = MemoryVault {
        unavailable: true,
        ..MemoryVault::default()
    };
    assert_eq!(
        save_verified(&directory, &unavailable, account.clone(), "mock-refresh").unwrap_err(),
        Error::VaultUnavailable
    );
    assert_eq!(
        super::super::load(&directory)
            .unwrap()
            .active()
            .unwrap()
            .name(),
        "OfflinePlayer"
    );

    let vault = MemoryVault::default();
    let accounts = save_verified(&directory, &vault, account.clone(), "mock-refresh").unwrap();
    assert_eq!(accounts.active(), Some(&account));
    assert_eq!(vault.read(UUID).unwrap().as_str(), "mock-refresh");
    let persisted = std::fs::read_to_string(directory.account_file()).unwrap();
    assert!(!persisted.contains("mock-refresh"));
    assert!(account.offline_session().is_none());

    let offline = &accounts.entries[0];
    let switched = super::super::select(&directory, offline).unwrap();
    assert_eq!(
        switched
            .active()
            .unwrap()
            .offline_session()
            .unwrap()
            .user_type,
        "legacy"
    );
    let removed = super::super::remove(&directory, offline).unwrap();
    assert!(removed.active().is_none());
    assert_eq!(removed.entries, [account]);
}
