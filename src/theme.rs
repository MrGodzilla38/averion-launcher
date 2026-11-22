use iced::{
    Background, Border, Color, Font, Theme, color, font,
    theme::Palette,
    widget::{button, container, text},
};

pub const BASE: Color = color!(0x0d141d);
pub const PANEL: Color = color!(0x101923);
pub const CARD: Color = color!(0x182634);
pub const HOVER: Color = color!(0x243447);
pub const SELECTED: Color = color!(0x18354b);
pub const EDGE: Color = color!(0x34475a);
pub const BLUE: Color = color!(0x579dff);
pub const PRIMARY_HOVER: Color = color!(0x78b3ff);
pub const PRIMARY_PRESSED: Color = color!(0x4289e3);
pub const CYAN: Color = color!(0x67def2);
pub const TEXT: Color = color!(0xf1f6fa);
pub const MUTED: Color = color!(0xbfcedb);
pub const SUB: Color = color!(0xa4b6c8);
pub const SUCCESS: Color = color!(0x82dea9);
pub const WARNING: Color = color!(0xf2c475);
pub const DANGER: Color = color!(0xff9b9b);
pub const GAP: u32 = 12;
pub const INSET: u32 = 16;
pub const RADIUS: f32 = 8.0;

pub fn dark() -> Theme {
    Theme::custom(
        "Averion",
        Palette {
            background: BASE,
            text: TEXT,
            primary: BLUE,
            success: SUCCESS,
            warning: WARNING,
            danger: DANGER,
        },
    )
}

pub fn bold(label: impl text::IntoFragment<'static>, size: u32) -> text::Text<'static> {
    text(label).size(size).font(Font {
        weight: font::Weight::Bold,
        ..Font::DEFAULT
    })
}

pub fn surface(fill: Color, edge: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(fill)),
        border: Border {
            color: edge,
            width: 1.,
            radius: radius.into(),
        },
        text_color: Some(TEXT),
        ..Default::default()
    }
}

pub fn control(selected: bool, focused: bool, status: button::Status) -> button::Style {
    let fill = match status {
        button::Status::Hovered => HOVER,
        button::Status::Pressed => SELECTED,
        _ if selected => SELECTED,
        _ => Color::TRANSPARENT,
    };
    button::Style {
        background: Some(fill.into()),
        text_color: if selected { CYAN } else { TEXT },
        border: Border {
            color: if focused {
                CYAN
            } else if selected {
                BLUE
            } else {
                Color::TRANSPARENT
            },
            width: if focused { 2. } else { 1. },
            radius: RADIUS.into(),
        },
        ..Default::default()
    }
}

/// The preblurred terrain shows through both gradient stops.
pub fn navigation_glass(arrival: f32) -> container::Style {
    let tint = iced::gradient::Linear::new(iced::Degrees(180.))
        .add_stop(0., HOVER.scale_alpha(0.45))
        .add_stop(1., PANEL.scale_alpha(0.55));
    let mut glass = surface(
        PANEL,
        if arrival < 1. {
            BLUE.scale_alpha(0.23 + 0.3 * (1. - arrival))
        } else {
            MUTED.scale_alpha(0.23)
        },
        16.,
    );
    glass.background = Some(tint.into());
    glass
}
