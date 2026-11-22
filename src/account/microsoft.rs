mod browser;

use super::{
    Account, Accounts, Session,
    vault::{Native, Vault},
};
use crate::{download::Cancellation, game_directory::GameDirectory, http::Https};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use std::{
    fmt, io,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const AUTHORIZE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const OAUTH_SCOPE: &str = "XboxLive.signin offline_access";
const CLIENT_ID_ENV: &str = "AVERION_MICROSOFT_CLIENT_ID";
const RESPONSE_LIMIT: usize = 256 * 1024;
const TOKEN_LIMIT: usize = 32 * 1024;
const EXPIRY_MARGIN: Duration = Duration::from_secs(30);

struct Endpoints {
    oauth: String,
    xbox: String,
    xsts: String,
    minecraft: String,
    entitlements: String,
    profile: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            oauth: "https://login.microsoftonline.com/consumers/oauth2/v2.0/token".into(),
            xbox: "https://user.auth.xboxlive.com/user/authenticate".into(),
            xsts: "https://xsts.auth.xboxlive.com/xsts/authorize".into(),
            minecraft: "https://api.minecraftservices.com/authentication/login_with_xbox".into(),
            entitlements: "https://api.minecraftservices.com/entitlements/mcstore".into(),
            profile: "https://api.minecraftservices.com/minecraft/profile".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Browser,
    Microsoft,
    Xbox,
    Minecraft,
    Ownership,
    Profile,
    Saving,
}

impl Stage {
    pub fn status(self) -> &'static str {
        match self {
            Self::Browser => "Finish signing in in your browser…",
            Self::Microsoft => "Confirming Microsoft sign-in…",
            Self::Xbox => "Signing in to Xbox…",
            Self::Minecraft => "Signing in to Minecraft Services…",
            Self::Ownership => "Checking Minecraft Java Edition ownership…",
            Self::Profile => "Loading your Minecraft profile…",
            Self::Saving => "Saving your Microsoft account securely…",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingClientId,
    InvalidClientId,
    Random,
    OpenBrowser,
    BrowserCallback,
    CallbackTimeout,
    Cancelled,
    Authorization,
    Network(Stage),
    Response(Stage),
    Rejected(Stage, u16),
    XboxProfile,
    XboxRegion,
    XboxAge,
    AppRegistration,
    NoOwnership,
    NoProfile,
    WrongProfile,
    SignInAgain,
    VaultUnavailable,
    VaultCapacity,
    AccountFile(io::ErrorKind),
    Worker,
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::AccountFile(error.kind())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingClientId | Self::InvalidClientId => {
                let problem = match self {
                    Self::MissingClientId => "Microsoft sign-in requires an Application (client) ID.",
                    _ => "AVERION_MICROSOFT_CLIENT_ID must be a hyphenated UUID.",
                };
                write!(f, "{problem} Set AVERION_MICROSOFT_CLIENT_ID to the ID of a Microsoft Entra desktop app registered for personal Microsoft accounts, with the http://localhost redirect URI and Minecraft Java Game Service API approval. Then restart Averion. See README for setup.")
            }
            Self::Random => f.write_str("Could not generate secure Microsoft sign-in values."),
            Self::OpenBrowser => f.write_str("Could not open the default web browser. Check your system’s default browser and try again."),
            Self::BrowserCallback => f.write_str("Could not open or use the local Microsoft sign-in callback. Check that localhost is available and try again."),
            Self::CallbackTimeout => f.write_str("Averion did not receive the Microsoft sign-in callback. Check the browser for errors and confirm that http://localhost is registered as a desktop redirect URI, then try again."),
            Self::Cancelled => f.write_str("Microsoft sign-in cancelled."),
            Self::Authorization => f.write_str("Microsoft declined sign-in. Check the app registration and try again."),
            Self::Network(stage) => write!(f, "Could not reach {}. Check your network connection and try again.", stage.service()),
            Self::Response(stage) => write!(f, "{} returned invalid authentication data.", stage.service()),
            Self::Rejected(stage, status) => write!(f, "{} rejected the request (HTTP {status}).", stage.service()),
            Self::XboxProfile => f.write_str("This Microsoft account needs an Xbox profile. Sign in at xbox.com to create one."),
            Self::XboxRegion => f.write_str("Xbox services are unavailable in this account’s region."),
            Self::XboxAge => f.write_str("Xbox requires family settings or age verification for this account."),
            Self::AppRegistration => f.write_str("Minecraft Services rejected this application. Check its Minecraft Java API approval."),
            Self::NoOwnership => f.write_str("This account does not have a Minecraft Java Edition entitlement."),
            Self::NoProfile => f.write_str("Create your Minecraft Java profile on minecraft.net and sign in again."),
            Self::WrongProfile => f.write_str("This sign-in returned a different Minecraft profile. Sign in again with the intended account."),
            Self::SignInAgain => f.write_str("Your Microsoft session has expired or was revoked. Sign in again in Accounts."),
            Self::VaultUnavailable => f.write_str("Secure credential storage is unavailable. Unlock or start your OS credential service and try again."),
            Self::VaultCapacity => f.write_str("The OS credential store could not fit the Microsoft refresh token."),
            Self::AccountFile(kind) => write!(f, "Could not update saved accounts ({kind})."),
            Self::Worker => f.write_str("Could not finish the account operation."),
        }
    }
}

impl Stage {
    fn service(self) -> &'static str {
        match self {
            Self::Microsoft | Self::Browser | Self::Saving => "Microsoft",
            Self::Xbox => "Xbox services",
            Self::Minecraft | Self::Ownership | Self::Profile => "Minecraft Services",
        }
    }
}

pub(crate) fn client_id() -> Result<String, Error> {
    let runtime = std::env::var_os(CLIENT_ID_ENV);
    resolve_client_id(
        runtime.as_deref(),
        option_env!("AVERION_MICROSOFT_CLIENT_ID"),
    )
}

fn resolve_client_id(
    runtime: Option<&std::ffi::OsStr>,
    build: Option<&str>,
) -> Result<String, Error> {
    let value = match runtime {
        Some(runtime) => runtime.to_str().ok_or(Error::InvalidClientId)?,
        None => build.ok_or(Error::MissingClientId)?,
    };
    super::canonical_uuid(value)
        .filter(|_| value.len() == 36)
        .ok_or(Error::InvalidClientId)
}

#[derive(Deserialize)]
struct Secret(#[serde(deserialize_with = "secret")] Zeroizing<String>);

fn secret<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Zeroizing<String>, D::Error> {
    let value = Zeroizing::new(String::deserialize(deserializer)?);
    if valid_token(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("invalid token"))
    }
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= TOKEN_LIMIT
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Deserialize)]
struct OAuthTokens {
    access_token: Secret,
    refresh_token: Option<Secret>,
    token_type: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct XboxToken {
    #[serde(rename = "Token")]
    token: Secret,
    #[serde(rename = "DisplayClaims")]
    claims: Claims,
}

#[derive(Deserialize)]
struct Claims {
    xui: Vec<UserClaim>,
}

#[derive(Deserialize)]
struct UserClaim {
    uhs: String,
    #[serde(default)]
    xid: String,
}

impl XboxToken {
    fn user(&self) -> Result<&UserClaim, Error> {
        match self.claims.xui.as_slice() {
            [user]
                if !user.uhs.is_empty()
                    && user.uhs.len() <= 32
                    && user.uhs.bytes().all(|byte| byte.is_ascii_alphanumeric())
                    && user.xid.len() <= 20
                    && user.xid.bytes().all(|byte| byte.is_ascii_digit()) =>
            {
                Ok(user)
            }
            _ => Err(Error::Response(Stage::Xbox)),
        }
    }
}

#[derive(Deserialize)]
struct GameToken {
    access_token: Secret,
    token_type: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct Entitlements {
    items: Vec<Entitlement>,
}

#[derive(Deserialize)]
struct Entitlement {
    name: String,
}

#[derive(Deserialize)]
struct Profile {
    id: String,
    name: String,
}

struct Verified {
    account: Account,
    token: Secret,
    expires_at: Instant,
    xuid: String,
}

impl Verified {
    fn session(self, client_id: &str) -> Result<Session, Error> {
        if self.expires_at.saturating_duration_since(Instant::now()) <= EXPIRY_MARGIN {
            return Err(Error::Response(Stage::Minecraft));
        }
        Ok(Session {
            player_name: self.account.name,
            uuid: self.account.uuid,
            access_token: self.token.0,
            user_type: "msa",
            client_id: client_id.to_owned(),
            xuid: self.xuid,
        })
    }
}

struct Services {
    client: Client,
    endpoints: Endpoints,
}

impl Services {
    fn new() -> Result<Self, Error> {
        Ok(Self {
            client: Https::default()
                .client()
                .map_err(|_| Error::Network(Stage::Microsoft))?
                .clone(),
            endpoints: Endpoints::default(),
        })
    }

    async fn exchange(
        &self,
        client_id: &str,
        grant: &[(&str, &str)],
    ) -> Result<OAuthTokens, Error> {
        let mut form = vec![("client_id", client_id), ("scope", OAUTH_SCOPE)];
        form.extend_from_slice(grant);
        let tokens = request::<OAuthTokens>(
            self.client.post(&self.endpoints.oauth).form(&form),
            Stage::Microsoft,
        )
        .await?;
        expiration(
            tokens.expires_in,
            &tokens.token_type,
            Stage::Microsoft,
            Instant::now(),
        )?;
        Ok(tokens)
    }

    async fn verify(
        &self,
        oauth_access: &Secret,
        mut progress: impl FnMut(Stage),
    ) -> Result<Verified, Error> {
        progress(Stage::Xbox);
        let ticket = Zeroizing::new(format!("d={}", oauth_access.0.as_str()));
        let user: XboxToken = request(self.client.post(&self.endpoints.xbox)
            .header("x-xbl-contract-version", "1")
            .json(&json!({
                "Properties": {"AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": ticket.as_str()},
                "RelyingParty": "http://auth.xboxlive.com",
                "TokenType": "JWT"
            })), Stage::Xbox).await?;
        let xsts: XboxToken = request(
            self.client
                .post(&self.endpoints.xsts)
                .header("x-xbl-contract-version", "1")
                .json(&json!({
                    "Properties": {"SandboxId": "RETAIL", "UserTokens": [user.token.0.as_str()]},
                    "RelyingParty": "rp://api.minecraftservices.com/",
                    "TokenType": "JWT"
                })),
            Stage::Xbox,
        )
        .await?;
        let claim = xsts.user()?;
        let xbox_user = user.user()?;
        if claim.uhs != xbox_user.uhs {
            return Err(Error::Response(Stage::Xbox));
        }
        let identity = Zeroizing::new(format!("XBL3.0 x={};{}", claim.uhs, xsts.token.0.as_str()));
        progress(Stage::Minecraft);
        let issued_at = Instant::now();
        let game: GameToken = request(
            self.client
                .post(&self.endpoints.minecraft)
                .json(&json!({"identityToken": identity.as_str()})),
            Stage::Minecraft,
        )
        .await?;
        let expires_at = expiration(
            game.expires_in,
            &game.token_type,
            Stage::Minecraft,
            issued_at,
        )?;

        progress(Stage::Ownership);
        let entitlements: Entitlements = request(
            self.client
                .get(&self.endpoints.entitlements)
                .bearer_auth(game.access_token.0.as_str()),
            Stage::Ownership,
        )
        .await?;
        if !entitlements
            .items
            .iter()
            .any(|item| matches!(item.name.as_str(), "game_minecraft" | "product_minecraft"))
        {
            return Err(Error::NoOwnership);
        }
        progress(Stage::Profile);
        let profile: Profile = request(
            self.client
                .get(&self.endpoints.profile)
                .bearer_auth(game.access_token.0.as_str()),
            Stage::Profile,
        )
        .await?;
        let account =
            Account::microsoft(profile.name, &profile.id).ok_or(Error::Response(Stage::Profile))?;
        Ok(Verified {
            account,
            token: game.access_token,
            expires_at,
            xuid: if claim.xid.is_empty() {
                xbox_user.xid.clone()
            } else {
                claim.xid.clone()
            },
        })
    }
}

fn expiration(
    seconds: u64,
    token_type: &str,
    stage: Stage,
    issued_at: Instant,
) -> Result<Instant, Error> {
    if !token_type.eq_ignore_ascii_case("bearer") || Duration::from_secs(seconds) <= EXPIRY_MARGIN {
        return Err(Error::Response(stage));
    }
    issued_at
        .checked_add(Duration::from_secs(seconds))
        .ok_or(Error::Response(stage))
}

async fn request<T: DeserializeOwned>(builder: RequestBuilder, stage: Stage) -> Result<T, Error> {
    let mut response = builder
        .timeout(Duration::from_secs(25))
        .send()
        .await
        .map_err(|_| Error::Network(stage))?;
    let status = response.status();
    let mut bytes = Zeroizing::new(Vec::new());
    if response
        .content_length()
        .is_some_and(|size| size > RESPONSE_LIMIT as u64)
    {
        return Err(Error::Response(stage));
    }
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Network(stage))? {
        if chunk.len() > RESPONSE_LIMIT - bytes.len() {
            return Err(Error::Response(stage));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        return Err(rejection(stage, status, &bytes));
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Response(stage))
}

fn rejection(stage: Stage, status: StatusCode, body: &[u8]) -> Error {
    #[derive(Default, Deserialize)]
    struct Rejection<'a> {
        error: Option<&'a str>,
        #[serde(rename = "XErr")]
        xbox_error: Option<u64>,
    }
    let code = serde_json::from_slice::<Rejection<'_>>(body).unwrap_or_default();
    match (stage, status) {
        (Stage::Microsoft, _) if code.error == Some("invalid_grant") => Error::SignInAgain,
        (Stage::Xbox, _) => match code.xbox_error {
            Some(2148916233) => Error::XboxProfile,
            Some(2148916235) => Error::XboxRegion,
            Some(2148916236..=2148916238) => Error::XboxAge,
            _ => Error::Rejected(stage, status.as_u16()),
        },
        (Stage::Minecraft, StatusCode::FORBIDDEN) => Error::AppRegistration,
        (Stage::Profile, StatusCode::NOT_FOUND) => Error::NoProfile,
        (Stage::Ownership | Stage::Profile, StatusCode::UNAUTHORIZED) => Error::SignInAgain,
        _ => Error::Rejected(stage, status.as_u16()),
    }
}

pub(crate) async fn sign_in(
    client_id: &str,
    cancel: Cancellation,
    mut progress: impl FnMut(Stage),
) -> Result<Accounts, Error> {
    let (account, refresh) = cancel
        .wait(async {
            let browser = browser::Browser::bind().await?;
            let code = browser
                .authorize(client_id, || progress(Stage::Browser))
                .await?;
            progress(Stage::Microsoft);
            let services = Services::new()?;
            let tokens = services
                .exchange(
                    client_id,
                    &[
                        ("grant_type", "authorization_code"),
                        ("code", &code),
                        ("redirect_uri", browser.redirect()),
                        ("code_verifier", browser.verifier()),
                    ],
                )
                .await?;
            let refresh = tokens
                .refresh_token
                .ok_or(Error::Response(Stage::Microsoft))?;
            let verified = services.verify(&tokens.access_token, &mut progress).await?;
            Ok::<_, Error>((verified.account, refresh))
        })
        .await
        .map_err(|_| Error::Cancelled)??;
    cancel.check().map_err(|_| Error::Cancelled)?;
    progress(Stage::Saving);
    blocking(move || {
        let directory = GameDirectory::discover()?;
        save_verified(&directory, &Native, account, &refresh.0)
    })
    .await
}

fn save_verified(
    directory: &GameDirectory,
    vault: &impl Vault,
    account: Account,
    refresh: &str,
) -> Result<Accounts, Error> {
    let _lock = directory.lock_accounts()?;
    let mut accounts = super::load(directory)?;
    let new_account = accounts.index_of(&account).is_none();
    let uuid = account.uuid.clone();
    accounts.insert(account)?;
    vault.write(&uuid, refresh)?;
    if let Err(error) = super::write(directory, &accounts) {
        if new_account {
            // The new vault entry has no public account referencing it.
            let _ = vault.remove(&uuid);
        }
        return Err(error.into());
    }
    Ok(accounts)
}

pub(crate) async fn launch_session(
    directory: &GameDirectory,
    account: Account,
) -> Result<Session, Error> {
    if let Some(session) = account.offline_session() {
        return Ok(session);
    }
    let client_id = client_id()?;
    let uuid = account.uuid.clone();
    let directory = directory.clone();
    // Serialize rotation with account removal and another launcher refreshing
    // the same account. Holding the file lock does not occupy an executor thread.
    let (lock, refresh) = blocking({
        let uuid = uuid.clone();
        let directory = directory.clone();
        move || {
            let lock = directory.lock_accounts()?;
            if super::load(&directory)?
                .active()
                .is_none_or(|active| !active.same_identity(&account))
            {
                return Err(Error::WrongProfile);
            }
            Ok((lock, Native.read(&uuid)?))
        }
    })
    .await?;
    let services = Services::new()?;
    let (verified, rotated) = refresh_session(&services, &client_id, &uuid, &refresh).await?;
    let profile = verified.account.clone();
    let session = verified.session(&client_id)?;
    blocking(move || {
        let _lock = lock;
        if let Some(rotated) = rotated {
            Native.write(&uuid, &rotated.0)?;
        }
        let mut accounts = super::load(&directory)?;
        let index = accounts.index_of(&profile).ok_or(Error::WrongProfile)?;
        if accounts.entries[index] != profile {
            accounts.entries[index] = profile;
            super::write(&directory, &accounts)?;
        }
        Ok(())
    })
    .await?;
    Ok(session)
}

async fn refresh_session(
    services: &Services,
    client_id: &str,
    uuid: &str,
    refresh: &str,
) -> Result<(Verified, Option<Secret>), Error> {
    if !valid_token(refresh) {
        return Err(Error::SignInAgain);
    }
    let tokens = services
        .exchange(
            client_id,
            &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
        )
        .await?;
    let verified = services.verify(&tokens.access_token, |_| {}).await?;
    if verified.account.uuid != uuid {
        return Err(Error::WrongProfile);
    }
    Ok((verified, tokens.refresh_token))
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| Error::Worker)?
}

#[cfg(test)]
mod tests;
