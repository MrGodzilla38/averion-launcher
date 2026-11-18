use super::*;
use iced::widget::column;

pub(super) fn sources(compact: bool) -> View {
    let source = |name, body| {
        container(column![bold(name, 15), text(body).size(14).color(SUB)].spacing(3)).width(Fill)
    };
    let minecraft = source("Minecraft", "Official news feed unavailable.");
    let averion = source("Averion", "No project announcements.");
    if compact {
        column![minecraft, averion].spacing(8).into()
    } else {
        row![minecraft, averion].spacing(24).into()
    }
}

pub(super) fn view(state: &State) -> View {
    let (title, description, content): (_, _, View) = match state.page {
        Page::Versions => {
            let columns = version_columns(state.compact);
            let cards = crate::catalog::families().enumerate().map(|(i, entry)| {
                let selected = entry.id == VERSIONS[state.selected].cover;
                let cover = image(art::version_art(entry.cover).card.clone())
                    .width(Fill)
                    .height(if state.compact { 102 } else { 148 })
                    .content_fit(ContentFit::Fill)
                    .border_radius(6);
                action(
                    state,
                    Control::VersionFamily(i),
                    column![
                        cover,
                        row![
                            bold(entry.id, 20),
                            Space::new().width(Fill),
                            text(if selected { "Selected" } else { "" })
                                .size(13)
                                .color(CYAN)
                        ]
                        .align_y(Alignment::Center),
                        text(entry.title).size(15)
                    ]
                    .spacing(6),
                    selected,
                )
                .width(Fill)
                .into()
            });
            let cards: Vec<View> = cards.collect();
            let mut grid = column![].spacing(INSET);
            let mut cards = cards.into_iter().peekable();
            while cards.peek().is_some() {
                let mut card_row = row![].spacing(INSET);
                for _ in 0..columns {
                    card_row = card_row.push(
                        cards
                            .next()
                            .unwrap_or_else(|| Space::new().width(Fill).into()),
                    );
                }
                grid = grid.push(card_row);
            }
            (
                "Versions",
                "Choose a version family to see its releases.",
                grid.into(),
            )
        }
        Page::Mods => (
            "Mods",
            "Planned for a future release.",
            note(
                "Vanilla Minecraft",
                "Mod loaders and mod management are unavailable.",
            ),
        ),
        Page::Accounts => (
            "Accounts",
            "Manage the player profile used by the launcher.",
            row![
                art::icon(8, 42),
                note(
                    "No account connected",
                    "Sign-in and account switching are unavailable."
                )
            ]
            .spacing(INSET)
            .align_y(Alignment::Center)
            .into(),
        ),
        Page::News => (
            "News",
            "Minecraft updates and Averion project announcements.",
            sources(state.compact),
        ),
        Page::Friends => (
            "Social / Friends",
            "Planned for a future release.",
            note(
                "Friends",
                "Player presence and friend lists are unavailable.",
            ),
        ),
        Page::Settings => (
            "Settings",
            "Appearance and accessibility for this session.",
            column![
                action(
                    state,
                    Control::Motion,
                    row![
                        bold("Reduced motion", 17),
                        Space::new().width(Fill),
                        text(if state.motion { "Off" } else { "On" }).color(CYAN)
                    ]
                    .align_y(Alignment::Center),
                    !state.motion
                )
                .width(Fill),
                text("Disables the navigation highlight animation until the launcher closes.")
                    .size(15)
                    .color(SUB),
                note(
                    "About Averion",
                    concat!(
                        "Version ",
                        env!("CARGO_PKG_VERSION"),
                        "  ·  Rust / Iced  ·  tiny-skia"
                    )
                )
            ]
            .spacing(INSET)
            .into(),
        ),
        Page::Home => unreachable!("Home has its own stage"),
    };
    let panel = container(content)
        .padding(20)
        .width(Fill)
        .style(|_| surface(CARD, EDGE, 12.));
    container(column![bold(title, 32), text(description).color(MUTED), panel].spacing(INSET))
        .max_width(940)
        .padding([24, 16])
        .center_x(Fill)
        .into()
}
fn note(title: &'static str, body: &'static str) -> View {
    column![bold(title, 18), text(body).size(15).color(SUB)]
        .spacing(6)
        .into()
}

pub(super) fn version_popup(state: &State, index: usize) -> View {
    use crate::catalog::{families, patches};
    use iced::widget::{mouse_area, opaque};

    let family = families().nth(index).unwrap();
    let header = row![
        bold(family.id, 24),
        text(family.title).size(15).color(SUB),
        Space::new().width(Fill),
        action(state, Control::CloseVersions, text("Close"), false)
    ]
    .spacing(10)
    .align_y(Alignment::Center);
    let releases = row(patches(family.id).map(|(i, release)| {
        action(
            state,
            Control::Version(i),
            container(text(release.id)).center_x(Fill),
            i == state.selected,
        )
        .width(114)
        .into()
    }))
    .spacing(8)
    .wrap();
    let panel = container(column![header, releases].spacing(14))
        .width(Fill)
        .max_width(432)
        .padding(20)
        .style(|_| surface(PANEL, EDGE, 12.));
    let backdrop =
        mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| {
            container::Style::default().background(iced::Color::BLACK.scale_alpha(0.65))
        }))
        .on_press(Message::Press(Control::CloseVersions));

    stack![
        opaque(backdrop),
        container(opaque(panel)).center(Fill).padding(16)
    ]
    .width(Fill)
    .height(Fill)
    .into()
}
