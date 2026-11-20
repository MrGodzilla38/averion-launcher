mod app;
mod catalog;
pub mod download;
mod game_directory;
mod http;
mod message;
mod mojang;
mod theme;
mod ui;

pub use app::Launcher;
pub use message::Message;

pub fn run() -> iced::Result {
    iced::application(Launcher::default, Launcher::update, Launcher::view)
        .font(include_bytes!("../assets/fonts/Minecraft.ttf").as_slice())
        .title("Averion Launcher")
        .theme(theme::dark())
        .subscription(Launcher::subscription)
        .window(iced::window::Settings {
            size: iced::Size::new(1120.0, 720.0),
            min_size: Some(iced::Size::new(640.0, 440.0)),
            exit_on_close_request: false,
            ..Default::default()
        })
        .run()
}
