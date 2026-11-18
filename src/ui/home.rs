use super::*;
use iced::widget::column;

fn version_preview(state: &State) -> View {
    let entry = &VERSIONS[state.selected];
    let (width, height) = if state.compact {
        (320, 160)
    } else {
        (440, 220)
    };
    let artwork = image(art::version_art(entry.cover).card.clone())
        .content_fit(ContentFit::Fill)
        .width(Fill)
        .height(Fill);
    let digits = |ink| {
        text(entry.id)
            .size(if state.compact { 24 } else { 32 })
            .font(iced::Font {
                weight: iced::font::Weight::Medium,
                ..iced::Font::with_name("Minecraft")
            })
            .color(ink)
    };
    let label = stack![
        container(digits(BASE)).padding(iced::Padding::new(0.).top(2).left(2)),
        digits(iced::Color::WHITE),
    ];
    let card = stack![
        artwork,
        container(label)
            .width(Fill)
            .padding([8, 12])
            .align_bottom(Fill)
            .style(|_| {
                let fade = iced::gradient::Linear::new(iced::Degrees(180.))
                    .add_stop(0.55, BASE.scale_alpha(0.))
                    .add_stop(1., BASE.scale_alpha(0.80));
                container::Style::default().background(fade)
            })
    ]
    .clip(true);
    let focused = state.focus == Control::VersionPreview;
    let preview = button(card)
        .on_press(Message::Press(Control::VersionPreview))
        .width(width)
        .height(height)
        .padding(3)
        .clip(true)
        .style(move |_, status| {
            let mut style = control(false, focused, status);
            let rim = match status {
                button::Status::Hovered => CYAN,
                button::Status::Pressed => PRIMARY_PRESSED,
                _ => BLUE,
            };
            style.background = Some(rim.into());
            style
        });
    container(preview).center_x(Fill).into()
}

pub(super) fn view(state: &State) -> View {
    let entry = &VERSIONS[state.selected];
    let compact = state.compact;
    let head = row![
        column![
            text("MINECRAFT  /  JAVA EDITION").size(13).color(CYAN),
            bold(entry.title, if compact { 24 } else { 34 })
        ]
        .spacing(4),
        Space::new().width(Fill),
        text("Local preview").size(14).color(MUTED)
    ]
    .align_y(Alignment::End);
    let hero = column![head, version_preview(state)].spacing(if compact { 6 } else { 16 });
    let hero = container(hero)
        .padding(if compact { [10, 16] } else { [20, 28] })
        .width(Fill);
    let details = action(
        state,
        Control::Details,
        text(if state.details_visible() {
            "Hide details (F1)"
        } else {
            "Startup details (F1)"
        })
        .size(14),
        false,
    );
    let mut actions = column![
        launch_control(
            compact,
            "▷   Launch",
            "Game launching is unavailable in this preview.",
            None
        ),
        details
    ]
    .align_x(Alignment::Center)
    .spacing(4);
    if state.details_visible() {
        actions = actions.push(
            text(concat!(
                "Native Rust application started.   •   v",
                env!("CARGO_PKG_VERSION"),
                "   •   Interface: Iced   /   Renderer: tiny-skia"
            ))
            .size(14),
        );
    }
    let actions = container(actions)
        .width(Fill)
        .center_x(Fill)
        .padding([8, 12]);
    let news = action(state, Control::News, text("View news  ›").size(14), false);
    let announcements = container(
        column![
            row![bold("Announcements", 18), Space::new().width(Fill), news]
                .align_y(Alignment::Center),
            super::pages::sources(compact)
        ]
        .spacing(4),
    )
    .padding([8, INSET as u16])
    .width(Fill)
    .style(|_| surface(PANEL.scale_alpha(0.95), EDGE, 12.));
    let landscape = container(column![hero, actions])
        .width(Fill)
        .style(|_| surface(BASE.scale_alpha(0.48), EDGE, 12.));
    container(column![landscape, announcements].spacing(if compact { 6 } else { GAP }))
        .max_width(1180)
        .center_x(Fill)
        .into()
}

/// Presentation only. The workflow owner supplies its stage label, explanation
/// and an action only when that operation is available.
fn launch_control(
    compact: bool,
    label: &'static str,
    status: &'static str,
    intent: Option<Message>,
) -> View {
    let action = button(container(bold(label, 20)).center(Fill))
        .width(if compact { 210 } else { 246 })
        .height(if compact { 44 } else { 52 })
        .on_press_maybe(intent)
        .style(|_, state| {
            let (fill, ink) = match state {
                button::Status::Disabled => (SELECTED, MUTED),
                button::Status::Hovered => (PRIMARY_HOVER, PANEL),
                button::Status::Pressed => (PRIMARY_PRESSED, PANEL),
                button::Status::Active => (BLUE, PANEL),
            };
            button::Style {
                background: Some(fill.into()),
                text_color: ink,
                border: iced::Border {
                    color: BLUE,
                    width: 1.,
                    radius: RADIUS.into(),
                },
                ..Default::default()
            }
        });
    column![action, text(status).size(14).color(MUTED)]
        .spacing(4)
        .align_x(Alignment::Center)
        .into()
}
