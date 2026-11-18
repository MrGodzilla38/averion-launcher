use crate::{
    catalog::{VERSIONS, families, patches},
    message::{Control, Page},
};

pub struct State {
    pub page: Page,
    pub selected: usize,
    pub focus: Control,
    pub compact: bool,
    pub menu: bool,
    pub version_family: Option<usize>,
    pub motion: bool,
    details_visible: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            page: Page::Home,
            selected: VERSIONS
                .iter()
                .position(|entry| entry.id == "1.20")
                .unwrap_or(0),
            focus: Control::VersionPreview,
            compact: false,
            menu: false,
            version_family: None,
            motion: true,
            details_visible: false,
        }
    }
}

impl State {
    pub fn details_visible(&self) -> bool {
        self.details_visible
    }
    pub fn toggle_details(&mut self) {
        self.details_visible = !self.details_visible;
    }

    pub fn tabs(&self) -> Vec<Control> {
        if let Some(family) = self.version_family {
            let cover = families().nth(family).unwrap().id;
            return std::iter::once(Control::CloseVersions)
                .chain(patches(cover).map(|(i, _)| Control::Version(i)))
                .collect();
        }
        let mut order = Vec::with_capacity(14);
        if self.compact {
            order.extend([Control::Nav(Page::Home), Control::Menu, Control::Profile]);
            if self.menu {
                order.extend(Page::ALL[1..].iter().copied().map(Control::Nav));
            }
        } else {
            order.extend(Page::ALL.map(Control::Nav));
            order.push(Control::Profile);
        }
        match self.page {
            Page::Home => order.extend([Control::VersionPreview, Control::Details, Control::News]),
            Page::Versions => order.extend((0..families().count()).map(Control::VersionFamily)),
            Page::Settings => order.push(Control::Motion),
            _ => {}
        }
        order
    }
    pub fn cycle(&mut self, backwards: bool) {
        let order = self.tabs();
        let index = order
            .iter()
            .position(|item| *item == self.focus)
            .unwrap_or(0);
        let next = if backwards {
            (index + order.len() - 1) % order.len()
        } else {
            (index + 1) % order.len()
        };
        self.focus = order[next];
    }
    pub fn resize(&mut self, compact: bool) {
        if compact != self.compact {
            self.compact = compact;
            self.menu = false;
            if !self.tabs().contains(&self.focus) {
                self.focus = if compact {
                    Control::Menu
                } else {
                    Control::Nav(self.page)
                };
            }
        }
    }
    pub fn navigate(&mut self, page: Page) {
        self.page = page;
        self.version_family = None;
        self.menu = false;
        self.focus = match page {
            Page::Home => Control::VersionPreview,
            Page::Versions => Control::VersionFamily(
                families()
                    .position(|family| family.id == VERSIONS[self.selected].cover)
                    .unwrap(),
            ),
            Page::Settings => Control::Motion,
            Page::Accounts => Control::Profile,
            _ if self.compact => Control::Menu,
            _ => Control::Nav(page),
        };
    }
    pub fn press(&mut self, control: Control) {
        if self.version_family.is_some() && !self.tabs().contains(&control) {
            return;
        }
        match control {
            Control::Nav(page) => self.navigate(page),
            Control::Profile => self.navigate(Page::Accounts),
            Control::News => self.navigate(Page::News),
            Control::Menu => {
                self.focus = control;
                self.menu = !self.menu;
            }
            Control::VersionFamily(i) => {
                if self.page == Page::Versions
                    && let Some(family) = families().nth(i)
                {
                    self.version_family = Some(i);
                    self.focus = if VERSIONS[self.selected].cover == family.id {
                        Control::Version(self.selected)
                    } else {
                        Control::Version(patches(family.id).next().unwrap().0)
                    };
                }
            }
            Control::CloseVersions => {
                if let Some(family) = self.version_family.take() {
                    self.focus = Control::VersionFamily(family);
                }
            }
            Control::Version(i) => {
                if self.version_family.is_none() {
                    return;
                }
                self.selected = i;
                self.navigate(Page::Home);
            }
            Control::Details => {
                self.focus = control;
                self.toggle_details();
            }
            Control::Motion => {
                self.focus = control;
                self.motion = !self.motion;
            }
            Control::VersionPreview => self.navigate(Page::Versions),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::State;

    #[test]
    fn version_entry_points_share_route_focus_and_selection() {
        use crate::message::{Control, Page};
        for entry in [Control::VersionPreview, Control::Nav(Page::Versions)] {
            let mut state = State::default();
            let initial = state.selected;
            assert_eq!(super::VERSIONS[initial].id, "1.20");
            let initial_family = super::families()
                .position(|family| family.id == "1.20")
                .unwrap();
            let nether_family = super::families()
                .position(|family| family.id == "1.16")
                .unwrap();
            state.press(entry);
            assert_eq!(
                (state.page, state.focus),
                (Page::Versions, Control::VersionFamily(initial_family))
            );
            let patch = super::VERSIONS
                .iter()
                .position(|version| version.id == "1.16.5")
                .unwrap();
            state.press(Control::VersionFamily(nether_family));
            assert!(state.tabs().contains(&Control::Version(patch)));
            assert!(!state.tabs().contains(&Control::Version(initial)));
            state.press(Control::CloseVersions);
            assert_eq!(state.focus, Control::VersionFamily(nether_family));
            assert_eq!(state.selected, initial);
            state.press(Control::VersionFamily(nether_family));
            state.press(Control::Version(patch));
            assert_eq!(
                (state.page, state.focus, state.selected),
                (Page::Home, Control::VersionPreview, patch)
            );
            assert_eq!(super::VERSIONS[state.selected].cover, "1.16");
            state.press(Control::Version(0));
            assert_eq!(state.selected, patch);
        }
    }

    #[test]
    fn compact_menu_restores_reachable_focus() {
        use crate::message::{Control, Page};
        let mut state = State::default();
        state.navigate(Page::Friends);
        state.resize(true);
        assert_eq!(state.focus, Control::Menu);
        state.press(Control::Menu);
        assert!(state.tabs().contains(&Control::Nav(Page::Friends)));
        state.cycle(true);
        assert!(state.tabs().contains(&state.focus));
        state.navigate(Page::Settings);
        assert!(!state.menu);
        state.press(Control::Motion);
        assert!(!state.motion);
    }

    #[test]
    fn details_start_hidden() {
        assert!(!State::default().details_visible());
    }

    #[test]
    fn toggles_alternate_details_visibility() {
        let mut state = State::default();
        for expected in [true, false, true, false] {
            state.toggle_details();
            assert_eq!(state.details_visible(), expected);
        }
    }
}
