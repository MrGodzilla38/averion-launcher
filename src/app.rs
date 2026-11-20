pub(crate) mod background;
pub(crate) mod installation;
pub(crate) mod state;

use crate::{
    catalog::VERSIONS,
    download::Download,
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
    close_after_download: Option<window::Id>,
}

impl Launcher {
    /// Starts a validated plan from an installation workflow.
    pub fn start_downloads(&mut self, plan: Vec<Download>) -> Task<Message> {
        self.state
            .installation
            .start(Arc::from(plan))
            .map_or_else(Task::none, |events| {
                Task::run(events, Message::Installation)
            })
    }

    fn download_selected(&mut self) -> Task<Message> {
        if self.state.page != Page::Home {
            return Task::none();
        }
        let version = VERSIONS[self.state.selected].id;
        let Some(events) = self
            .state
            .installation
            .start_selected(version, self.mojang.clone())
        else {
            return Task::none();
        };
        self.state.verified_versions.remove(version);
        self.state.focus = Control::DownloadAction;
        Task::run(events, Message::Installation)
    }

    fn download_action(&mut self) -> Task<Message> {
        if self.state.page != Page::Home {
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
            Message::Press(Control::StartDownload) => return self.download_selected(),
            Message::Press(Control::DownloadAction) => return self.download_action(),
            Message::Press(control) => self.state.press(control),
            Message::Focus(backwards) => self.state.cycle(backwards),
            Message::Activate if self.state.focus == Control::StartDownload => {
                return self.download_selected();
            }
            Message::Activate if self.state.focus == Control::DownloadAction => {
                return self.download_action();
            }
            Message::Activate => self.state.press(self.state.focus),
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
                return window::scale_factor(id).map(Message::Rescale);
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
                if self.state.installation.is_active() {
                    self.close_after_download = Some(id);
                    self.state.installation.cancel();
                    return Task::none();
                }
                return window::close(id);
            }
            Message::Installation(event) => {
                let finished = matches!(event, installation::Event::Finished(_));
                if let Some((version, runtime)) = self.state.installation.update(event) {
                    self.state.verified_versions.insert(version, runtime);
                }
                if self.state.focus == Control::DownloadAction
                    && !self.state.installation.has_action()
                {
                    self.state.focus = Control::Details;
                }
                if finished && let Some(id) = self.close_after_download.take() {
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
