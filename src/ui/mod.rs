pub(crate) mod art;
mod home;
pub mod input;
mod pages;
mod profile;

use crate::{
    app::state::State,
    catalog::VERSIONS,
    message::{Control, Message, Page},
    theme::*,
};
use iced::{
    Alignment, ContentFit, Element, Fill, Length,
    widget::{Space, button, column, container, image, row, scrollable, stack, text},
};

type View = Element<'static, Message>;

pub fn version_columns(compact: bool) -> usize {
    if compact { 2 } else { 3 }
}

fn action(
    state: &State,
    id: Control,
    content: impl Into<View>,
    selected: bool,
) -> button::Button<'static, Message> {
    let focused = state.focus == id;
    button(content)
        .padding([7, 9])
        .on_press(Message::Press(id))
        .style(move |_, status| control(selected, focused, status))
}

fn nav_item(state: &State, page: Page) -> View {
    action(
        state,
        Control::Nav(page),
        row![art::icon(page as usize, 16), text(page.label()).size(14)]
            .spacing(5)
            .align_y(Alignment::Center),
        state.page == page,
    )
    .into()
}

fn navigation(state: &State, arrival: f32) -> View {
    let mut links = row![].spacing(2).align_y(Alignment::Center);
    if state.compact {
        let secondary_selected = Page::NAVIGATION[1..].contains(&state.page);
        links = links.push(nav_item(state, Page::Home)).push(action(
            state,
            Control::Menu,
            text(if state.menu {
                "Close  ×"
            } else if secondary_selected {
                state.page.label()
            } else {
                "Menu  ▾"
            })
            .size(15),
            state.menu || secondary_selected,
        ));
    } else {
        links = links.extend(Page::NAVIGATION.map(|p| nav_item(state, p)));
    }
    let brand = row![art::icon(7, 28), bold("Averion", 21)]
        .align_y(Alignment::Center)
        .spacing(6);
    let profile = action(
        state,
        Control::Profile,
        row![
            art::icon(8, 24),
            column![
                bold("Profile", 14),
                text(
                    state
                        .accounts
                        .active()
                        .map_or("Signed out", |account| account.auth().label())
                )
                .size(12)
                .color(SUB)
            ]
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        state.page == Page::Accounts,
    );
    let top = row![
        brand,
        Space::new().width(Fill),
        links,
        Space::new().width(Fill),
        profile
    ]
    .align_y(Alignment::Center);
    let mut nav = column![top];
    if state.menu && state.compact {
        nav = nav.push(
            row(Page::NAVIGATION[1..].iter().map(|p| nav_item(state, *p)))
                .spacing(2)
                .wrap(),
        );
    }
    container(nav)
        .padding([6, 12])
        .width(Fill)
        .style(move |_| navigation_glass(arrival))
        .into()
}

pub fn shell(state: &State, arrival: f32, background: Option<image::Handle>) -> View {
    let page = if state.page == Page::Home {
        home::view(state)
    } else {
        pages::view(state)
    };
    let body = scrollable(container(page).center_x(Fill))
        .width(Fill)
        .id("body")
        .height(Fill);
    let layout = column![navigation(state, arrival), body]
        .spacing(8)
        .padding(if state.compact { 10 } else { 18 })
        .width(Fill)
        .height(Fill);
    // Secondary page headings sit directly over the terrain.
    let scene_opacity: f32 = if state.page == Page::Home { 1. } else { 0.7 };
    let sky = if let Some(handle) = background {
        image(handle)
            .filter_method(image::FilterMethod::Nearest)
            .content_fit(ContentFit::Fill)
    } else {
        image(
            art::version_art(VERSIONS[state.selected].cover)
                .sky_handle
                .clone(),
        )
        .content_fit(ContentFit::Cover)
    };
    let sky = sky.width(Fill).height(Fill).opacity(scene_opacity);
    let mut layers = stack![sky, layout].width(Length::Fill).height(Fill);
    if let Some(family) = state.version_family {
        layers = layers.push(pages::version_popup(state, family));
    }
    layers.into()
}
