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
    pub const NAVIGATION: [Self; 6] = [
        Self::Home,
        Self::Versions,
        Self::Mods,
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
    UpdateFabric,
    OpenModsFolder,
    BrowseMods,
    OfflineName,
    SaveOffline,
    MicrosoftSignIn,
    CancelMicrosoftSignIn,
    SelectAccount(usize),
    RemoveAccount(usize),
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
    LocalReady(Vec<&'static str>, Result<crate::account::Accounts, String>),
    ModsOpened(Result<(), String>),
    OfflineNameChanged(String),
    AccountProgress(crate::account::microsoft::Stage),
    AccountsUpdated(Result<crate::account::Accounts, String>, &'static str),
    ProfileSkinReady(
        u64,
        Result<iced::widget::image::Handle, crate::account::skin::Error>,
    ),
    Launch(crate::launch::Event),
    WindowOpened(iced::window::Id, iced::Size),
    Resize(iced::Size),
    Rescale(f32),
    BackgroundReady(
        crate::app::background::Key,
        Option<iced::widget::image::Handle>,
    ),
    Frame(std::time::Instant),
}
