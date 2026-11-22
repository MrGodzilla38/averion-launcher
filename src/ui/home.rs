use super::*;
use crate::{app::installation::Installation, download::Stage};
use iced::widget::{column, progress_bar};

const MINECRAFT_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Medium,
    ..iced::Font::with_name("Minecraft")
};

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
            .font(MINECRAFT_FONT)
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
    let mut actions = column![launch_control(state), details]
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

fn launch_button_label(state: &State, version: &str) -> (&'static str, iced::Color) {
    let launch = &state.launch;
    if state.loading_local {
        ("LOADING", MUTED)
    } else if state.installation.is_active() {
        ("INSTALLING", WARNING)
    } else if launch.active() {
        if launch.running {
            ("RUNNING", SUCCESS)
        } else {
            ("STARTING", WARNING)
        }
    } else if launch.version == Some(version) && launch.repair {
        ("REPAIR", DANGER)
    } else if state.installed.contains(version) {
        ("PLAY", SUCCESS)
    } else {
        ("DOWNLOAD", BLUE)
    }
}

fn launch_control(state: &State) -> View {
    let width = if state.compact { 210 } else { 246 };
    let version = VERSIONS[state.selected].id;
    let installed = state.installed.contains(version);
    let launch = &state.launch;
    let (label, accent) = launch_button_label(state, version);
    let label = text(label).size(20).font(MINECRAFT_FONT);
    let focused = state.focus == Control::StartDownload;
    let main = button(container(label).center(Fill))
        .width(width)
        .height(if state.compact { 44 } else { 52 })
        .on_press_maybe(
            state
                .home_action_enabled()
                .then_some(Message::Press(Control::StartDownload)),
        )
        .style(move |_, status| launch_button_style(accent, focused, status));
    let controls = column![main].spacing(4).align_x(Alignment::Center);
    let controls = match &state.installation {
        Installation::Idle => {
            let status = if launch.active() || launch.version == Some(version) {
                if state.closing_after_game {
                    format!(
                        "{} Averion will close after Minecraft exits.",
                        launch.status
                    )
                } else {
                    launch.status.clone()
                }
            } else if state.loading_local {
                "Reading local installations…".into()
            } else if installed && state.accounts.active().is_none() {
                "Choose or add an account under Profile to play.".into()
            } else if installed {
                format!("Minecraft {version} with Fabric is installed.")
            } else {
                "Download the selected Minecraft release with Fabric.".into()
            };
            controls.push(text(status).size(14).color(MUTED))
        }
        Installation::Failed { .. } if launch.active() => {
            controls.push(text(launch.status.clone()).size(14).color(MUTED))
        }
        Installation::Failed { error, .. } => controls
            .push(text(error.to_string()).size(14).color(MUTED))
            .push(download_action(state, "RETRY", None)),
        Installation::Active {
            progress, cancel, ..
        } => {
            let step = if progress.count > 1
                && (progress.overall.is_none() || progress.stage == Stage::Extracting)
            {
                format!(" ({}/{})", progress.step, progress.count)
            } else {
                String::new()
            };
            let status = if cancel.is_none() {
                format!("Cancelling {}…", progress.component)
            } else {
                match progress.stage {
                    Stage::Resolving => format!("Resolving {} metadata…", progress.component),
                    Stage::Extracting => format!("Extracting {}…{step}", progress.component),
                    Stage::Checking => format!("Checking {}…{step}", progress.component),
                    Stage::Receiving { .. } => format!("Downloading {}…{step}", progress.component),
                }
            };
            let indicator =
                if let Some(overall) = progress.overall.filter(|totals| totals.total_bytes > 0) {
                    format!(
                        "{}% total · {}/{} files verified",
                        u128::from(overall.bytes) * 100 / u128::from(overall.total_bytes),
                        overall.files,
                        overall.total_files,
                    )
                } else {
                    match progress.stage {
                        Stage::Receiving {
                            received,
                            total: Some(total),
                        } if total > 0 => {
                            format!("{}%", u128::from(received) * 100 / u128::from(total))
                        }
                        Stage::Receiving { received, .. } => format!("{received} bytes received"),
                        Stage::Resolving | Stage::Checking | Stage::Extracting => String::new(),
                    }
                };
            let bar: Option<View> = if let Some(percent) = progress.percent() {
                Some(
                    progress_bar(0. ..=100., percent)
                        .girth(6)
                        .length(width)
                        .into(),
                )
            } else if !matches!(progress.stage, Stage::Resolving) {
                Some(
                    container(Space::new())
                        .height(6)
                        .width(width)
                        .style(|_| surface(SELECTED, EDGE, 3.))
                        .into(),
                )
            } else {
                None
            };
            controls
                .extend(bar)
                .push(text(status).size(14).color(MUTED))
                .extend(
                    (!indicator.is_empty())
                        .then(|| View::from(text(indicator).size(13).color(MUTED))),
                )
                .extend(
                    cancel
                        .is_some()
                        .then(|| download_action(state, "CANCEL", Some(DANGER))),
                )
        }
    };
    controls.into()
}

fn launch_button_style(
    accent: iced::Color,
    focused: bool,
    status: button::Status,
) -> button::Style {
    let blend = |base: iced::Color, amount: f32| {
        iced::Color::from_rgb(
            accent.r + (base.r - accent.r) * amount,
            accent.g + (base.g - accent.g) * amount,
            accent.b + (base.b - accent.b) * amount,
        )
    };
    let (fill, ink) = match status {
        button::Status::Disabled => (blend(PANEL, 0.85), accent),
        button::Status::Hovered if accent == BLUE => (PRIMARY_HOVER, PANEL),
        button::Status::Pressed if accent == BLUE => (PRIMARY_PRESSED, PANEL),
        button::Status::Hovered => (blend(iced::Color::WHITE, 0.18), PANEL),
        button::Status::Pressed => (blend(PANEL, 0.15), PANEL),
        button::Status::Active => (accent, PANEL),
    };
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: iced::Border {
            color: if focused { CYAN } else { accent },
            width: if focused { 2. } else { 1. },
            radius: RADIUS.into(),
        },
        ..Default::default()
    }
}

fn download_action(state: &State, label: &'static str, accent: Option<iced::Color>) -> View {
    let control = action(
        state,
        Control::DownloadAction,
        text(label).size(14).font(MINECRAFT_FONT),
        false,
    )
    .height(44);
    match accent {
        Some(accent) => {
            let focused = state.focus == Control::DownloadAction;
            control
                .style(move |_, status| launch_button_style(accent, focused, status))
                .into()
        }
        None => control.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_button_pairs_labels_with_activity_colors() {
        let mut state = State::default();
        let version = VERSIONS[state.selected].id;
        assert_eq!(launch_button_label(&state, version), ("DOWNLOAD", BLUE));

        state.loading_local = true;
        assert_eq!(launch_button_label(&state, version), ("LOADING", MUTED));
        state.loading_local = false;

        let installation =
            state
                .installation
                .start_selected(version, false, crate::mojang::Mojang::default());
        assert_eq!(
            launch_button_label(&state, version),
            ("INSTALLING", WARNING)
        );
        drop(installation);
        state.installation = Installation::Idle;
        state.installed.insert(version);
        assert_eq!(launch_button_label(&state, version), ("PLAY", SUCCESS));

        state.launch.start(version);
        assert_eq!(launch_button_label(&state, version), ("STARTING", WARNING));
        state.launch.running = true;
        assert_eq!(launch_button_label(&state, version), ("RUNNING", SUCCESS));

        state.launch.active = false;
        state.launch.repair = true;
        assert_eq!(launch_button_label(&state, version), ("REPAIR", DANGER));
    }
}
