use super::*;
use crate::app::state::MICROSOFT_SIGN_IN_ENABLED;
use iced::widget::{column, text_input};

pub(super) fn view(state: &State) -> View {
    let ready = state.home_action_enabled();
    let sign_in = if state.sign_in_cancellable {
        action(
            state,
            Control::CancelMicrosoftSignIn,
            text("Cancel sign-in"),
            false,
        )
    } else {
        let sign_in = action(
            state,
            Control::MicrosoftSignIn,
            text("Sign in with Microsoft"),
            true,
        );
        if MICROSOFT_SIGN_IN_ENABLED {
            sign_in
        } else {
            sign_in.on_press_maybe(None).style(|_, status| {
                let mut style = control(true, false, status);
                style.background = Some(PANEL.into());
                style.border.color = EDGE;
                style.text_color = SUB;
                style
            })
        }
    };
    let mut microsoft = column![sign_in.height(44)].spacing(4);
    if !MICROSOFT_SIGN_IN_ENABLED {
        microsoft = microsoft.push(text("Temporarily disabled").size(14).color(SUB));
    } else if state.microsoft_client_id.is_err() && state.account_status.is_empty() {
        microsoft = microsoft.push(
            text("Microsoft sign-in needs an approved application ID. See README for setup.")
                .size(14)
                .color(SUB),
        );
    }

    let mut content = column![bold("Profile", 34), microsoft].spacing(16);
    if !state.account_status.is_empty() {
        content = content.push(section(
            text(state.account_status.clone()).size(14).color(MUTED),
        ));
    }
    let mut saved = column![bold("Saved accounts", 18)].spacing(8);
    for (index, account) in state.accounts.entries.iter().enumerate() {
        let active = state.accounts.is_active(index);
        let select = Control::SelectAccount(index);
        let remove = Control::RemoveAccount(index);
        let mut item = row![
            column![
                bold(account.name().to_owned(), 16),
                text(if active {
                    format!("{} · Active", account.auth().label())
                } else {
                    account.auth().label().into()
                })
                .size(14)
                .color(if active { CYAN } else { SUB })
            ]
            .spacing(3)
            .width(Fill),
        ]
        .align_y(Alignment::Center)
        .spacing(8);
        if !active {
            item = item.push(
                action(state, select, text("Use account"), false)
                    .height(44)
                    .on_press_maybe(ready.then_some(Message::Press(select))),
            );
        }
        saved = saved.push(section(
            item.push(
                action(state, remove, text("Remove"), false)
                    .height(44)
                    .on_press_maybe(ready.then_some(Message::Press(remove))),
            ),
        ));
    }
    if !state.accounts.entries.is_empty() {
        content = content.push(saved);
    }

    let mut input = text_input("Enter a Minecraft username", &state.account_input)
        .id("offline-name")
        .padding(12)
        .width(Length::Fill);
    if ready {
        input = input
            .on_input(Message::OfflineNameChanged)
            .on_submit(Message::Press(Control::SaveOffline));
    }
    let offline = column![
        bold("Offline username", 18),
        input,
        text("Use 3–16 letters, numbers, or underscores.")
            .size(14)
            .color(SUB),
        action(
            state,
            Control::SaveOffline,
            text("Save offline profile"),
            true
        )
        .height(44)
        .on_press_maybe(ready.then_some(Message::Press(Control::SaveOffline))),
    ]
    .spacing(8);
    let compact = state.compact;
    let width = if compact { 176 } else { 320 };
    let mut portrait = column![art::profile_portrait(width, state.skin.image())]
        .width(width)
        .align_x(Alignment::Center);
    if let Some(status) = state.skin.status() {
        portrait = portrait.push(text(status).center().size(14).color(SUB));
    }
    let profile = row![portrait, content.push(section(offline)).width(Fill)].spacing(if compact {
        14
    } else {
        32
    });
    container(
        container(profile)
            .padding(if compact { 16 } else { 28 })
            .width(Fill)
            .style(|_| surface(BASE.scale_alpha(0.94), EDGE, 16.)),
    )
    .max_width(940)
    .padding([18, 12])
    .center_x(Fill)
    .into()
}

fn section(content: impl Into<View>) -> View {
    container(content)
        .padding(12)
        .width(Fill)
        .style(|_| surface(PANEL, EDGE, 10.))
        .into()
}
