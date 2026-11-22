use super::{Error, OAUTH_SCOPE};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::{net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    process::Command,
    time::timeout,
};
use zeroize::Zeroizing;

const CALLBACK_LIMIT: usize = 16 * 1024;
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(180);

pub(super) struct Browser {
    listener: TcpListener,
    redirect: Url,
    verifier: Zeroizing<String>,
    state: Zeroizing<String>,
}

impl Browser {
    pub async fn bind() -> Result<Self, Error> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| Error::BrowserCallback)?;
        let port = listener
            .local_addr()
            .map_err(|_| Error::BrowserCallback)?
            .port();
        let redirect =
            Url::parse(&format!("http://localhost:{port}/")).expect("valid loopback URL");
        Ok(Self {
            listener,
            redirect,
            verifier: random_value()?,
            state: random_value()?,
        })
    }

    pub fn redirect(&self) -> &str {
        self.redirect.as_str()
    }

    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    fn authorization_url(&self, client_id: &str) -> Url {
        let mut url = Url::parse(super::AUTHORIZE).expect("valid authorization URL");
        url.query_pairs_mut().extend_pairs([
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", self.redirect()),
            ("response_mode", "query"),
            ("scope", OAUTH_SCOPE),
            ("state", self.state.as_str()),
            ("code_challenge", &challenge(self.verifier())),
            ("code_challenge_method", "S256"),
            ("prompt", "select_account"),
        ]);
        url
    }

    pub async fn authorize(
        &self,
        client_id: &str,
        on_open: impl FnOnce(),
    ) -> Result<Zeroizing<String>, Error> {
        #[cfg(target_os = "linux")]
        let command = Command::new("xdg-open");
        #[cfg(windows)]
        let command = {
            let mut command = Command::new("rundll32.exe");
            command.arg("url.dll,FileProtocolHandler");
            command
        };
        self.authorize_with(command, client_id, on_open).await
    }

    async fn authorize_with(
        &self,
        mut command: Command,
        client_id: &str,
        on_open: impl FnOnce(),
    ) -> Result<Zeroizing<String>, Error> {
        let url = self.authorization_url(client_id);
        let mut browser = command
            .arg(url.as_str())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|_| Error::OpenBrowser)?;
        on_open();
        timeout(SIGN_IN_TIMEOUT, async {
            // Some Linux desktop handlers run until the browser closes.
            match futures::future::select(Box::pin(self.receive()), Box::pin(browser.wait())).await
            {
                futures::future::Either::Left((callback, _)) => callback,
                futures::future::Either::Right((Ok(status), receiver)) if status.success() => {
                    receiver.await
                }
                futures::future::Either::Right(_) => Err(Error::OpenBrowser),
            }
        })
        .await
        .map_err(|_| Error::CallbackTimeout)?
    }

    async fn receive(&self) -> Result<Zeroizing<String>, Error> {
        loop {
            let (mut peer, _) = self
                .listener
                .accept()
                .await
                .map_err(|_| Error::BrowserCallback)?;
            if let Ok(Some(result)) = timeout(Duration::from_secs(2), self.handle(&mut peer)).await
            {
                return result;
            }
        }
    }

    async fn handle(&self, peer: &mut TcpStream) -> Option<Result<Zeroizing<String>, Error>> {
        let mut input = Zeroizing::new(Vec::new());
        let mut chunk = Zeroizing::new([0; 1024]);
        while !input.windows(4).any(|window| window == b"\r\n\r\n") {
            let read = peer.read(chunk.as_mut()).await.ok()?;
            if read == 0 || input.len() + read > CALLBACK_LIMIT {
                return None;
            }
            input.extend_from_slice(&chunk[..read]);
        }
        let result = callback(
            std::str::from_utf8(&input).ok()?,
            &self.redirect,
            &self.state,
        );
        let (status, message) = match &result {
            Some(Ok(_)) => (
                "200 OK",
                "Continue in Averion Launcher. You can close this tab.",
            ),
            Some(Err(_)) => (
                "200 OK",
                "Microsoft sign-in was not completed. Return to Averion Launcher.",
            ),
            None => (
                "400 Bad Request",
                "This request did not match an Averion sign-in.",
            ),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n{message}",
            message.len()
        );
        // The code is still usable if the browser closes before receiving the response.
        let _ = peer.write_all(response.as_bytes()).await;
        result
    }
}

fn random_value() -> Result<Zeroizing<String>, Error> {
    let mut bytes = Zeroizing::new([0; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|_| Error::Random)?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref())))
}

fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn callback(
    request: &str,
    redirect: &Url,
    expected_state: &str,
) -> Option<Result<Zeroizing<String>, Error>> {
    let (headers, _) = request.split_once("\r\n\r\n")?;
    let mut lines = headers.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let ("GET", target, "HTTP/1.1", None) =
        (first.next()?, first.next()?, first.next()?, first.next())
    else {
        return None;
    };
    let mut hosts = lines.filter_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("host").then_some(value.trim())
    });
    let expected_host = format!("localhost:{}", redirect.port()?);
    if hosts.next()? != expected_host || hosts.next().is_some() || !target.starts_with("/?") {
        return None;
    }
    let url = redirect.join(target).ok()?;
    if url.fragment().is_some() || url.path() != redirect.path() {
        return None;
    }
    let mut state = None;
    let mut code = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        let value = Zeroizing::new(value.into_owned());
        let field = match key.as_ref() {
            "state" => &mut state,
            "code" => &mut code,
            "error" => &mut error,
            _ => continue,
        };
        if field.replace(value).is_some() {
            return None;
        }
    }
    if state?.as_str() != expected_state {
        return None;
    }
    match (code, error) {
        (Some(code), None) if !code.is_empty() => Some(Ok(code)),
        (None, Some(error)) if error.as_str() == "access_denied" => Some(Err(Error::Cancelled)),
        (None, Some(_)) => Some(Err(Error::Authorization)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIENT_ID: &str = "11111111-2222-3333-4444-555555555555";

    #[test]
    fn reports_browser_spawn_and_exit_failures() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let browser = Browser::bind().await.unwrap();
            let temp = tempfile::tempdir().unwrap();
            let missing = Command::new(temp.path().join("missing-browser-opener"));
            let result = browser
                .authorize_with(missing, CLIENT_ID, || panic!("opener did not start"))
                .await;
            assert_eq!(result.unwrap_err(), Error::OpenBrowser);

            let mut failing = Command::new(std::env::current_exe().unwrap());
            failing.arg("--invalid-test-option");
            let mut started = false;
            let result = browser
                .authorize_with(failing, CLIENT_ID, || started = true)
                .await;
            assert_eq!(result.unwrap_err(), Error::OpenBrowser);
            assert!(started);
        });
    }

    #[test]
    fn authorization_url_redirect_reaches_the_loopback_listener() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let browser = Browser::bind().await.unwrap();
            let url = browser.authorization_url(CLIENT_ID);
            let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(query["redirect_uri"], browser.redirect());
            assert_eq!(query["code_challenge_method"], "S256");
            let port = browser.redirect.port().unwrap();
            let request = format!(
                "GET /?state={}&code=test-code HTTP/1.1\r\nHost: localhost:{port}\r\n\r\n",
                query["state"]
            );
            let (code, ()) = timeout(Duration::from_secs(5), async {
                futures::join!(browser.receive(), async {
                    let mut peer = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
                        .await
                        .unwrap();
                    peer.write_all(request.as_bytes()).await.unwrap();
                    let mut response = String::new();
                    peer.read_to_string(&mut response).await.unwrap();
                    assert!(response.starts_with("HTTP/1.1 200 OK"));
                })
            })
            .await
            .unwrap();
            assert_eq!(code.unwrap().as_str(), "test-code");
        });
    }

    #[test]
    fn pkce_matches_rfc_7636_and_uses_independent_random_secrets() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let verifier = random_value().unwrap();
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier.as_str(), random_value().unwrap().as_str());
    }

    #[test]
    fn rejects_wrong_state_duplicate_parameters_paths_and_hosts() {
        let redirect = Url::parse("http://localhost:8765/").unwrap();
        let run = |query: &str, host: &str| {
            callback(
                &format!("GET {query} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
                &redirect,
                "expected",
            )
        };
        assert_eq!(
            run("/?state=expected&code=one%2Btwo", "localhost:8765")
                .unwrap()
                .unwrap()
                .as_str(),
            "one+two"
        );
        for query in [
            "/?state=other&code=a",
            "/?state=expected&state=expected&code=a",
            "/?state=expected&code=a&code=b",
            "/?state=expected&code=a&error=access_denied",
            "/wrong?state=expected&code=a",
            "//localhost:8765/?state=expected&code=a",
        ] {
            assert!(run(query, "localhost:8765").is_none(), "{query}");
        }
        assert!(run("/?state=expected&code=a", "other:8765").is_none());
        assert!(matches!(
            run("/?state=expected&error=access_denied", "localhost:8765"),
            Some(Err(Error::Cancelled))
        ));
    }
}
