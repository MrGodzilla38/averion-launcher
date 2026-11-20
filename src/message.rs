#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Home,
    Versions,
    Mods,
    Accounts,
    Settings,
    News,
    Friends,
}

impl Page {
    pub const ALL: [Self; 7] = [
        Self::Home,
        Self::Versions,
        Self::Mods,
        Self::Accounts,
        Self::Settings,
        Self::News,
        Self::Friends,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Versions => "Versions",
            Self::Mods => "Mods",
            Self::Accounts => "Accounts",
            Self::Settings => "Settings",
            Self::News => "News",
            Self::Friends => "Friends",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Nav(Page),
    Menu,
    Profile,
    VersionPreview,
    VersionFamily(usize),
    CloseVersions,
    Version(usize),
    StartDownload,
    DownloadAction,
    Details,
    Motion,
    News,
}

#[derive(Clone, Debug)]
pub enum Message {
    ToggleDetails,
    Press(Control),
    Focus(bool),
    Activate,
    Escape,
    CloseRequested(iced::window::Id),
    Installation(crate::app::installation::Event),
    WindowOpened(iced::window::Id, iced::Size),
    Resize(iced::Size),
    Rescale(f32),
    BackgroundReady(
        crate::app::background::Key,
        Option<iced::widget::image::Handle>,
    ),
    Frame(std::time::Instant),
}
