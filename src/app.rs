pub(crate) mod background;
mod game;
pub(crate) mod installation;
mod profile_skin;
pub(crate) mod state;

use crate::{
    catalog::VERSIONS,
    download::Download,
    launch,
    message::{Control, Message, Page},
    mojang::Mojang,
    ui,
};
use iced::{
    Element, Subscription, Task,
    widget::operation::{self, RelativeOffset},
    window,
};
use state::State;
use std::{sync::Arc, time::Instant};

pub const COMPACT_WIDTH: u32 = 1060;
const DURATION: f32 = 0.18;

struct Transition {
    progress: f32,
    start: Instant,
}

#[derive(Default)]
pub struct Launcher {
    state: State,
    transition: Option<Transition>,
    background: background::Background,
    mojang: Mojang,
    close_after_activity: Option<window::Id>,
    read_local_started: bool,
    sign_in_cancel: Option<futures::channel::oneshot::Sender<()>>,
}

enum AccountMutation {
    SaveOffline(String),
    Select(crate::account::Account),
    Remove(crate::account::Account),
}

impl Launcher {
    /// Starts a validated plan from an installation workflow.
    pub fn start_downloads(&mut self, plan: Vec<Download>) -> Task<Message> {
        if !self.state.home_action_enabled() {
            return Task::none();
        }
        self.state
            .installation
            .start(Arc::from(plan))
            .map_or_else(Task::none, |events| {
                Task::run(events, Message::Installation)
            })
    }

    fn home_action(&mut self) -> Task<Message> {
        if self.state.page != Page::Home || !self.state.home_action_enabled() {
            return Task::none();
        }
        let version = VERSIONS[self.state.selected].id;
        let repairing = self.state.launch.version == Some(version) && self.state.launch.repair;
        if self.state.installed.contains(version) && !repairing {
            if self.state.accounts.active().is_none() {
                self.state.focus = Control::Profile;
                return operation::snap_to("body", RelativeOffset::START);
            }
            self.state.launch.start(version);
            if self
                .state
                .accounts
                .active()
                .is_some_and(|account| account.auth() == crate::account::Auth::Microsoft)
            {
                self.state.launch.status =
                    format!("Checking Minecraft {version} files and Microsoft session…");
            }
            self.state.focus = Control::VersionPreview;
            return Task::run(launch::start(version), Message::Launch);
        }
        self.install_selected(version, repairing)
    }

    fn install_selected(&mut self, version: &'static str, repairing: bool) -> Task<Message> {
        let Some(events) =
            self.state
                .installation
                .start_selected(version, repairing, self.mojang.clone())
        else {
            return Task::none();
        };
        if repairing {
            self.state.installed.remove(version);
        }
        if self.state.launch.version == Some(version) {
            self.state.launch = game::Launch::default();
        }
        self.state.focus = Control::DownloadAction;
        Task::run(events, Message::Installation)
    }

    fn mods_action(&mut self, control: Control) -> Task<Message> {
        if self.state.page != Page::Mods {
            return Task::none();
        }
        let version = VERSIONS[self.state.selected].id;
        if control == Control::UpdateFabric {
            if !self.state.home_action_enabled() || !self.state.installed.contains(version) {
                return Task::none();
            }
            self.state.navigate(Page::Home);
            return self.install_selected(version, false);
        }
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    game::open_mods(version, control == Control::BrowseMods)
                })
                .await
                .unwrap_or_else(|_| Err("Could not open the mods location.".into()))
            },
            Message::ModsOpened,
        )
    }

    fn download_action(&mut self) -> Task<Message> {
        if self.state.page != Page::Home || self.state.launch.active() {
            return Task::none();
        }
        self.state.installation.cancel();
        let task = self
            .state
            .installation
            .retry()
            .map_or_else(Task::none, |events| {
                Task::run(events, Message::Installation)
            });
        self.state.focus = if self.state.installation.has_action() {
            Control::DownloadAction
        } else {
            Control::Details
        };
        task
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        let previous_page = self.state.page;
        let task = self.update_state(message);
        let name = self
            .state
            .accounts
            .active()
            .map(crate::account::Account::name);
        let skin = self.state.skin.sync(
            self.state.page == Page::Accounts && previous_page != Page::Accounts,
            name,
        );
        match skin {
            Some(skin) => Task::batch([task, skin]),
            None => task,
        }
    }

    fn update_state(&mut self, message: Message) -> Task<Message> {
        let previous = self.family();
        let page = self.state.page;
        match message {
            Message::ToggleDetails => {
                if self.state.page != Page::Home {
                    self.state.navigate(Page::Home);
                }
                self.state.toggle_details();
                self.state.focus = Control::Details;
            }
            Message::Press(Control::StartDownload) => return self.home_action(),
            Message::Press(Control::DownloadAction) => return self.download_action(),
            Message::Press(
                control @ (Control::SaveOffline
                | Control::MicrosoftSignIn
                | Control::CancelMicrosoftSignIn
                | Control::SelectAccount(_)
                | Control::RemoveAccount(_)),
            ) => return self.account_action(control),
            Message::Press(
                control @ (Control::UpdateFabric | Control::OpenModsFolder | Control::BrowseMods),
            ) => {
                return self.mods_action(control);
            }
            Message::Activate
                if matches!(
                    self.state.focus,
                    Control::UpdateFabric | Control::OpenModsFolder | Control::BrowseMods
                ) =>
            {
                return self.mods_action(self.state.focus);
            }
            Message::ModsOpened(result) => {
                self.state.mods_status = result.err().unwrap_or_default();
                return Task::none();
            }
            Message::Press(control) => self.state.press(control),
            Message::Focus(backwards) => {
                self.state.cycle(backwards);
                if self.state.page == Page::Accounts {
                    let target = if self.state.focus == Control::OfflineName {
                        "offline-name"
                    } else {
                        "unfocused"
                    };
                    let y = match self.state.focus {
                        Control::OfflineName | Control::SaveOffline => 1.,
                        Control::SelectAccount(i) | Control::RemoveAccount(i) => {
                            (i + 1) as f32 / (self.state.accounts.entries.len() + 1) as f32
                        }
                        _ => 0.,
                    };
                    return Task::batch([
                        operation::focus(target),
                        operation::snap_to("body", RelativeOffset { x: 0., y }),
                    ]);
                }
            }
            Message::Activate
                if matches!(
                    self.state.focus,
                    Control::SaveOffline
                        | Control::MicrosoftSignIn
                        | Control::CancelMicrosoftSignIn
                        | Control::SelectAccount(_)
                        | Control::RemoveAccount(_)
                ) =>
            {
                return self.account_action(self.state.focus);
            }
            Message::Activate if self.state.focus == Control::OfflineName => {
                return operation::focus("offline-name");
            }
            Message::OfflineNameChanged(name) => {
                self.state.focus = Control::OfflineName;
                self.state.account_input = name;
                if !self.state.account_busy {
                    self.state.account_status.clear();
                }
                return Task::none();
            }
            Message::AccountProgress(stage) => {
                if self.state.sign_in_cancellable
                    || stage == crate::account::microsoft::Stage::Saving
                {
                    self.state.account_status = stage.status().into();
                }
                if stage == crate::account::microsoft::Stage::Saving {
                    self.state.sign_in_cancellable = false;
                }
                return Task::none();
            }
            Message::AccountsUpdated(result, success) => {
                self.state.account_busy = false;
                self.state.sign_in_cancellable = false;
                self.sign_in_cancel.take();
                match result {
                    Ok(accounts) => {
                        self.state.accounts = accounts;
                        self.state.account_status = success.into();
                    }
                    Err(error) => self.state.account_status = error,
                }
                if !self.state.tabs().contains(&self.state.focus) {
                    self.state.focus = Control::Profile;
                }
                return self
                    .close_after_activity
                    .take()
                    .map_or_else(Task::none, window::close);
            }
            Message::ProfileSkinReady(request, result) => {
                self.state.skin.complete(request, result);
                return Task::none();
            }
            Message::LocalReady(versions, accounts) => {
                self.state.loading_local = false;
                self.state.installed.extend(versions);
                match accounts {
                    Ok(accounts) => {
                        if let Some(account) = accounts
                            .active()
                            .filter(|account| account.auth() == crate::account::Auth::Offline)
                        {
                            self.state.account_input = account.name().into();
                        }
                        self.state.accounts = accounts;
                    }
                    Err(error) => self.state.account_status = error,
                }
                return Task::none();
            }
            Message::Launch(event) => {
                if let launch::Event::Started {
                    profile: Some(profile),
                    ..
                } = &event
                {
                    self.state.accounts.update_profile(profile.clone());
                }
                self.state.launch.update(event);
                if !self.state.launch.active() {
                    self.state.closing_after_game = false;
                }
                if !self.state.launch.active()
                    && let Some(id) = self.close_after_activity.take()
                {
                    return window::close(id);
                }
                return Task::none();
            }
            Message::Activate if self.state.focus == Control::StartDownload => {
                return self.home_action();
            }
            Message::Activate if self.state.focus == Control::DownloadAction => {
                return self.download_action();
            }
            Message::Activate => self.state.press(self.state.focus),
            Message::Escape
                if self.state.page == Page::Accounts && self.state.sign_in_cancellable =>
            {
                self.cancel_sign_in();
                return Task::none();
            }
            Message::Escape => {
                if self.state.version_family.is_some() {
                    self.state.press(Control::CloseVersions);
                } else if self.state.menu {
                    self.state.menu = false;
                    self.state.focus = Control::Menu;
                }
            }
            Message::WindowOpened(id, size) => {
                self.background.size = size;
                self.state.resize(size.width < COMPACT_WIDTH as f32);
                let scale = window::scale_factor(id).map(Message::Rescale);
                if self.read_local_started {
                    return scale;
                }
                self.read_local_started = true;
                self.state.loading_local = true;
                let local = Task::perform(
                    async {
                        tokio::task::spawn_blocking(game::read_local)
                            .await
                            .unwrap_or_else(|_| {
                                (
                                    Vec::new(),
                                    Err("Could not read local launcher data.".into()),
                                )
                            })
                    },
                    |(versions, account)| Message::LocalReady(versions, account),
                );
                return Task::batch([scale, local]);
            }
            Message::Resize(size) => {
                self.background.size = size;
                self.state.resize(size.width < COMPACT_WIDTH as f32);
                return self.background.refresh(self.family());
            }
            Message::Rescale(scale) => {
                self.background.scale = Some(scale);
                return self.background.refresh(self.family());
            }
            Message::BackgroundReady(key, handle) => {
                return self.background.complete(key, handle, self.family());
            }
            Message::CloseRequested(id) => {
                if self.state.account_busy {
                    self.close_after_activity = Some(id);
                    self.cancel_sign_in();
                    return Task::none();
                }
                if self.state.launch.active() {
                    self.state.closing_after_game = true;
                    self.close_after_activity = Some(id);
                    self.state.navigate(Page::Home);
                    return Task::none();
                }
                if self.state.installation.is_active() {
                    self.close_after_activity = Some(id);
                    self.state.installation.cancel();
                    return Task::none();
                }
                return window::close(id);
            }
            Message::Installation(event) => {
                let finished = matches!(event, installation::Event::Finished(_));
                if let Some((version, _runtime)) = self.state.installation.update(event) {
                    self.state.installed.insert(version);
                    if self.state.launch.version == Some(version) {
                        self.state.launch = game::Launch::default();
                    }
                }
                if self.state.focus == Control::DownloadAction
                    && !self.state.installation.has_action()
                {
                    self.state.focus = Control::Details;
                }
                if finished && let Some(id) = self.close_after_activity.take() {
                    return window::close(id);
                }
                return Task::none();
            }
            Message::Frame(now) => {
                if let Some(anim) = &mut self.transition {
                    anim.progress = (now.saturating_duration_since(anim.start).as_secs_f32()
                        / DURATION)
                        .min(1.);
                    if anim.progress >= 1. {
                        self.transition = None;
                    }
                }
            }
        }
        if !self.state.motion {
            self.transition = None;
        } else if page != self.state.page {
            self.transition = Some(Transition {
                progress: 0.,
                start: Instant::now(),
            });
        }
        let refresh = if previous != self.family() {
            self.background.refresh(self.family())
        } else {
            Task::none()
        };
        if matches!(
            message,
            Message::ToggleDetails | Message::Focus(_) | Message::Activate | Message::Press(_)
        ) {
            let position = match (self.state.page, self.state.focus) {
                (Page::Home, Control::News | Control::Details) => 1.,
                (Page::Versions, Control::VersionFamily(i)) => {
                    let columns = ui::version_columns(self.state.compact);
                    let last_row = (crate::catalog::families().count() - 1) / columns;
                    (i / columns) as f32 / last_row.max(1) as f32
                }
                _ if page != self.state.page
                    || matches!(
                        self.state.focus,
                        Control::VersionPreview
                            | Control::Nav(_)
                            | Control::Menu
                            | Control::Profile
                    ) =>
                {
                    0.
                }
                _ => return refresh,
            };
            return Task::batch([
                refresh,
                operation::snap_to("body", RelativeOffset { x: 0., y: position }),
            ]);
        }
        refresh
    }
    fn account_action(&mut self, control: Control) -> Task<Message> {
        use crate::{account, game_directory::GameDirectory};

        if self.state.page != Page::Accounts {
            return Task::none();
        }
        if control == Control::CancelMicrosoftSignIn {
            self.cancel_sign_in();
            return Task::none();
        }
        if control == Control::MicrosoftSignIn {
            return if state::MICROSOFT_SIGN_IN_ENABLED {
                self.sign_in()
            } else {
                Task::none()
            };
        }
        if !self.state.home_action_enabled() {
            return Task::none();
        }
        let mutation = match control {
            Control::SaveOffline => {
                let name = self.state.account_input.clone();
                if let Err(error) = account::validate_name(&name) {
                    self.state.account_status = format!("Invalid username. {error}.");
                    return Task::none();
                }
                AccountMutation::SaveOffline(name)
            }
            Control::SelectAccount(index) | Control::RemoveAccount(index) => {
                let Some(account) = self.state.accounts.entries.get(index).cloned() else {
                    return Task::none();
                };
                if matches!(control, Control::SelectAccount(_)) {
                    AccountMutation::Select(account)
                } else {
                    AccountMutation::Remove(account)
                }
            }
            _ => return Task::none(),
        };
        self.state.focus = control;
        self.state.account_busy = true;
        self.state.account_status = "Updating saved accounts…".into();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    let directory = GameDirectory::discover()?;
                    match mutation {
                        AccountMutation::SaveOffline(name) => {
                            account::save_offline(&directory, &name)
                                .map(|accounts| (accounts, "Offline account saved."))
                                .map_err(Into::into)
                        }
                        AccountMutation::Select(account) => account::select(&directory, &account)
                            .map(|accounts| (accounts, "Active account changed."))
                            .map_err(Into::into),
                        AccountMutation::Remove(account) => account::remove(&directory, &account)
                            .map(|accounts| (accounts, "Account removed.")),
                    }
                })
                .await
                .unwrap_or(Err(account::microsoft::Error::Worker))
            },
            |result| match result {
                Ok((accounts, success)) => Message::AccountsUpdated(Ok(accounts), success),
                Err(error) => Message::AccountsUpdated(Err(error.to_string()), ""),
            },
        )
    }

    fn sign_in(&mut self) -> Task<Message> {
        use futures::SinkExt;
        if self.state.account_busy {
            return Task::none();
        }
        self.state.focus = Control::MicrosoftSignIn;
        let client_id = match &self.state.microsoft_client_id {
            Ok(client_id) => client_id.clone(),
            Err(error) => {
                self.state.account_status = error.to_string();
                return Task::none();
            }
        };
        if !self.state.home_action_enabled() {
            self.state.account_status =
                "Wait for the current launcher activity to finish, then try Microsoft sign-in again."
                    .into();
            return Task::none();
        }
        let (sender, cancel) = crate::download::cancellation();
        self.sign_in_cancel = Some(sender);
        self.state.account_busy = true;
        self.state.sign_in_cancellable = true;
        self.state.focus = Control::CancelMicrosoftSignIn;
        self.state.account_status = "Opening Microsoft sign-in…".into();
        Task::run(
            iced::stream::channel(
                8,
                move |mut events: futures::channel::mpsc::Sender<Message>| async move {
                    let result = crate::account::microsoft::sign_in(&client_id, cancel, |stage| {
                        let _ = events.try_send(Message::AccountProgress(stage));
                    })
                    .await
                    .map_err(|error| error.to_string());
                    let _ = events
                        .send(Message::AccountsUpdated(result, "Microsoft account saved."))
                        .await;
                },
            ),
            std::convert::identity,
        )
    }

    fn cancel_sign_in(&mut self) {
        if self.state.sign_in_cancellable
            && let Some(sender) = self.sign_in_cancel.take()
        {
            let _ = sender.send(());
            self.state.sign_in_cancellable = false;
            self.state.account_status = "Cancelling Microsoft sign-in…".into();
        }
    }

    fn family(&self) -> &'static str {
        VERSIONS[self.state.selected].cover
    }
    pub fn subscription(&self) -> Subscription<Message> {
        let keys = iced::keyboard::listen().filter_map(ui::input::shortcut);
        let resize = window::events().filter_map(|(id, event)| match event {
            window::Event::Opened { size, .. } => Some(Message::WindowOpened(id, size)),
            window::Event::Resized(size) => Some(Message::Resize(size)),
            window::Event::Rescaled(scale) => Some(Message::Rescale(scale)),
            window::Event::CloseRequested => Some(Message::CloseRequested(id)),
            _ => None,
        });
        if self.transition.is_some() {
            Subscription::batch([keys, resize, iced::window::frames().map(Message::Frame)])
        } else {
            Subscription::batch([keys, resize])
        }
    }
    pub fn view(&self) -> Element<'_, Message> {
        ui::shell(
            &self.state,
            self.transition
                .as_ref()
                .map_or(1., |transition| transition.progress),
            self.background.handle(self.family()),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::microsoft::{Error as SignInError, Stage as SignInStage};

    #[test]
    fn disabled_microsoft_sign_in_cannot_be_focused_or_activated() {
        let mut launcher = Launcher::default();
        launcher.state.navigate(Page::Accounts);
        launcher.state.microsoft_client_id = Ok("11111111-2222-3333-4444-555555555555".into());
        assert!(!launcher.state.tabs().contains(&Control::MicrosoftSignIn));
        assert_eq!(
            launcher
                .update(Message::Press(Control::MicrosoftSignIn))
                .units(),
            0
        );
        launcher.state.focus = Control::MicrosoftSignIn;
        assert_eq!(launcher.update(Message::Activate).units(), 0);
        assert!(!launcher.state.account_busy);
    }

    #[test]
    fn microsoft_sign_in_explains_configuration_errors() {
        for error in [SignInError::MissingClientId, SignInError::InvalidClientId] {
            let mut launcher = Launcher::default();
            launcher.state.navigate(Page::Accounts);
            launcher.state.microsoft_client_id = Err(error);

            assert_eq!(launcher.sign_in().units(), 0);
            assert_eq!(launcher.state.account_status, error.to_string());
            assert!(
                launcher
                    .state
                    .account_status
                    .contains("AVERION_MICROSOFT_CLIENT_ID")
            );
            assert!(launcher.state.account_status.contains("http://localhost"));
            assert!(!launcher.state.account_busy);
        }
    }

    #[test]
    fn microsoft_sign_in_shows_progress_and_startup_failure() {
        let mut launcher = Launcher::default();
        launcher.state.navigate(Page::Accounts);
        launcher.state.microsoft_client_id = Ok("11111111-2222-3333-4444-555555555555".into());
        launcher.state.loading_local = true;

        assert_eq!(launcher.sign_in().units(), 0);
        assert!(
            launcher
                .state
                .account_status
                .contains("current launcher activity")
        );
        let _ = launcher.update(Message::LocalReady(vec![], Ok(Default::default())));

        let task = launcher.sign_in();
        assert_ne!(task.units(), 0);
        assert_eq!(launcher.state.account_status, "Opening Microsoft sign-in…");
        assert!(launcher.state.account_busy);
        assert_eq!(launcher.state.focus, Control::CancelMicrosoftSignIn);
        let _ = launcher.update(Message::AccountProgress(SignInStage::Browser));
        assert_eq!(launcher.state.account_status, SignInStage::Browser.status());

        let error = SignInError::OpenBrowser.to_string();
        let _ = launcher.update(Message::AccountsUpdated(Err(error.clone()), ""));
        assert_eq!(launcher.state.account_status, error);
        assert!(!launcher.state.account_busy);
        assert!(!launcher.state.sign_in_cancellable);
    }

    #[test]
    fn mods_update_uses_the_existing_installation_workflow() {
        let mut launcher = Launcher::default();
        let _ = launcher.update(Message::LocalReady(
            vec!["1.20"],
            Ok(crate::account::Accounts::default()),
        ));
        let _ = launcher.update(Message::Press(Control::Nav(Page::Mods)));
        assert!(launcher.state.tabs().contains(&Control::UpdateFabric));

        let _ = launcher.update(Message::Press(Control::UpdateFabric));
        assert_eq!(launcher.state.page, Page::Home);
        assert!(launcher.state.installed.contains("1.20"));
        assert!(matches!(
            launcher.state.installation,
            installation::Installation::Active {
                request: installation::Request::Selected {
                    version: "1.20",
                    repair: false,
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn transitions_settle_and_reduced_motion_skips_them() {
        let mut launcher = Launcher::default();
        let _ = launcher.update(Message::Activate);
        assert_eq!(launcher.state.page, Page::Versions);
        let start = launcher.transition.as_ref().unwrap().start;
        let _ = launcher.update(Message::Frame(start + std::time::Duration::from_secs(1)));
        assert!(launcher.transition.is_none());
        let _ = launcher.update(Message::Press(Control::Nav(Page::Settings)));
        let _ = launcher.update(Message::Press(Control::Motion));
        let _ = launcher.update(Message::Press(Control::Nav(Page::Home)));
        let _ = launcher.update(Message::Activate);
        let _ = launcher.update(Message::Press(Control::VersionFamily(0)));
        let _ = launcher.update(Message::Press(Control::Version(0)));
        assert!(launcher.transition.is_none());
        assert_eq!(launcher.state.selected, 0);
    }

    #[test]
    fn startup_details_can_be_opened_and_dismissed() {
        let mut launcher = Launcher::default();
        assert!(!launcher.state.details_visible());
        let _ = launcher.update(Message::ToggleDetails);
        assert!(launcher.state.details_visible());
        let _ = launcher.update(Message::ToggleDetails);
        assert!(!launcher.state.details_visible());
    }

    #[test]
    fn home_keyboard_download_captures_the_exact_selected_version() {
        let mut launcher = Launcher::default();
        launcher.state.selected = VERSIONS
            .iter()
            .position(|version| version.id == "1.20.1")
            .unwrap();
        let _ = launcher.update(Message::Focus(false));
        assert_eq!(launcher.state.focus, Control::StartDownload);
        let _task = launcher.update(Message::Activate);
        assert_eq!(launcher.state.focus, Control::DownloadAction);
        assert!(matches!(
            &launcher.state.installation,
            installation::Installation::Active {
                request: installation::Request::Selected {
                    version: "1.20.1",
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn home_play_waits_for_an_account_and_prevents_duplicate_launches() {
        let mut launcher = Launcher::default();
        let _ = launcher.update(Message::LocalReady(
            vec!["1.20"],
            Ok(crate::account::Accounts::default()),
        ));
        let _ = launcher.update(Message::Press(Control::StartDownload));
        assert_eq!(launcher.state.page, Page::Home);
        assert_eq!(launcher.state.focus, Control::Profile);
        assert!(!launcher.state.launch.active());
        let _ = launcher.update(Message::Activate);
        assert_eq!(launcher.state.page, Page::Accounts);

        let base = tempfile::tempdir().unwrap();
        let directory = crate::game_directory::GameDirectory::for_test(base.path());
        let accounts = crate::account::save_offline(&directory, "OfflinePlayer").unwrap();
        let _ = launcher.update(Message::AccountsUpdated(
            Ok(accounts),
            "Offline account saved.",
        ));
        let _ = launcher.update(Message::Press(Control::Nav(Page::Home)));
        let _ = launcher.update(Message::Focus(false));
        let _ = launcher.update(Message::Activate);
        assert_eq!(launcher.state.launch.version, Some("1.20"));
        assert!(launcher.state.launch.active());
        assert!(!launcher.state.tabs().contains(&Control::StartDownload));
        let _ = launcher.update(Message::Press(Control::StartDownload));
        assert!(!launcher.state.installation.is_active());
        let _ = launcher.update(Message::Launch(launch::Event::Finished(Err(
            launch::Error::Installation {
                message: "Damaged".into(),
                repairable: true,
            },
        ))));
        assert!(launcher.state.launch.repair);
        assert!(!launcher.state.launch.active());
        assert!(launcher.state.tabs().contains(&Control::StartDownload));
    }

    #[test]
    fn keyboard_cancel_stays_active_until_transfer_cleanup() {
        use crate::download::{Response, Stage};
        use futures::{FutureExt, StreamExt, executor::block_on, io::AsyncRead};
        use std::{
            fs, io,
            pin::{Pin, pin},
            task::{Context, Poll},
        };

        struct Waiting;
        impl AsyncRead for Waiting {
            fn poll_read(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
                _: &mut [u8],
            ) -> Poll<io::Result<usize>> {
                Poll::Pending
            }
        }

        block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let destination = dir.path().join("artifact");
            let mut launcher = Launcher::default();
            let plan = Arc::from([Download {
                name: "Component".into(),
                destination: destination.clone(),
                expected_size: Some(3),
                checksum: None,
                existing: crate::download::ExistingFile::ReplaceInvalid,
                open: Box::new(|| {
                    async {
                        Ok(Response {
                            body: Box::new(Waiting),
                            total: Some(3),
                        })
                    }
                    .boxed()
                }),
            }]);
            let mut events = pin!(launcher.state.installation.start(plan).unwrap());
            assert!(launcher.state.tabs().contains(&Control::DownloadAction));

            // Cancel as soon as the task reports that staging has begun.
            while let Some(event) = events.next().await {
                let receiving = matches!(&event, installation::Event::Progress(progress)
                    if matches!(progress.stage, Stage::Receiving { received: 0, .. }));
                let _ = launcher.update(Message::Installation(event));
                if receiving {
                    break;
                }
            }
            assert!(!destination.exists());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
            let _ = launcher.update(Message::Focus(false));
            assert_eq!(launcher.state.focus, Control::DownloadAction);
            let _ = launcher.update(Message::Activate);
            assert!(matches!(
                launcher.state.installation,
                installation::Installation::Active { cancel: None, .. }
            ));
            assert!(!launcher.state.tabs().contains(&Control::DownloadAction));

            while let Some(event) = events.next().await {
                let _ = launcher.update(Message::Installation(event));
            }
            assert!(matches!(
                launcher.state.installation,
                installation::Installation::Idle
            ));
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        });
    }
}
