pub(crate) mod background;
pub(crate) mod state;

use crate::{
    catalog::VERSIONS,
    message::{Control, Message, Page},
    ui,
};
use iced::{
    Element, Subscription, Task,
    widget::operation::{self, RelativeOffset},
    window,
};
use state::State;
use std::time::Instant;

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
}

impl Launcher {
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
            Message::Press(control) => self.state.press(control),
            Message::Focus(backwards) => self.state.cycle(backwards),
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
}
