#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod app;
mod catalog;
mod message;
mod theme;
mod ui;

use app::Launcher;

fn main() -> iced::Result {
    iced::application(Launcher::default, Launcher::update, Launcher::view)
        .font(include_bytes!("../assets/fonts/Minecraft.ttf").as_slice())
        .title("Averion Launcher")
        .theme(theme::dark())
        .subscription(Launcher::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1120.0, 720.0),
            min_size: Some(iced::Size::new(640.0, 440.0)),
            ..Default::default()
        })
        .run()
}
