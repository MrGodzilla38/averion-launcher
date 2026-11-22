use crate::{account, catalog::VERSIONS, game_directory::GameDirectory, launch, mojang};

#[derive(Default)]
pub(crate) struct Launch {
    pub version: Option<&'static str>,
    pub status: String,
    pub active: bool,
    pub running: bool,
    pub repair: bool,
}

impl Launch {
    pub fn active(&self) -> bool {
        self.active
    }

    pub fn start(&mut self, version: &'static str) {
        *self = Self {
            version: Some(version),
            status: format!("Checking Minecraft {version} files and Java…"),
            active: true,
            running: false,
            repair: false,
        };
    }

    pub fn update(&mut self, event: launch::Event) {
        let Some(version) = self.version.filter(|_| self.active) else {
            return;
        };
        match event {
            launch::Event::Started { log } => {
                self.running = true;
                self.status = format!("Minecraft {version} is running. Output in {log}.");
            }
            launch::Event::Finished(result) => {
                self.active = false;
                self.running = false;
                self.status = match result {
                    Ok(exit) => {
                        let result = if exit.status.success() {
                            format!("Minecraft {version} closed normally.")
                        } else {
                            format!(
                                "Minecraft {version} exited with {}. See crash-reports for any game crash report.",
                                exit.status
                            )
                        };
                        let output = if exit.output_error {
                            "Output capture failed. See"
                        } else {
                            "Output in"
                        };
                        format!("{result} {output} {}.", exit.log)
                    }
                    Err(launch::Error::Installation {
                        message,
                        repairable,
                    }) => {
                        self.repair = repairable;
                        message
                    }
                    Err(launch::Error::Io(message) | launch::Error::Account(message)) => message,
                };
            }
        }
    }
}

pub(super) fn read_local() -> (Vec<&'static str>, Result<Option<String>, String>) {
    let Ok(directory) = GameDirectory::discover() else {
        return (
            Vec::new(),
            Err("Could not find the Averion data directory.".into()),
        );
    };
    let versions = VERSIONS
        .iter()
        .filter(|version| mojang::has_installation_record(&directory, version.id))
        .map(|version| version.id)
        .collect();
    let account = account::load(&directory)
        .map(|account| account.map(|account| account.name().to_owned()))
        .map_err(|error| format!("Could not read the offline account ({}).", error.kind()));
    (versions, account)
}

pub(super) fn save_account(name: &str) -> Result<String, String> {
    let directory = GameDirectory::discover().map_err(|error| error.to_string())?;
    account::save(&directory, name)
        .map(|account| account.name().to_owned())
        .map_err(|error| format!("Could not save the offline account ({error})."))
}

pub(super) fn open_mods(version: &str, browse: bool) -> Result<(), String> {
    use std::{ffi::OsStr, process::Command};

    let open = |target: &OsStr| {
        #[cfg(target_os = "linux")]
        let mut command = Command::new("xdg-open");
        #[cfg(windows)]
        let mut command = if browse {
            Command::new("rundll32.exe")
        } else {
            Command::new("explorer.exe")
        };
        #[cfg(windows)]
        if browse {
            command.arg("url.dll,FileProtocolHandler");
        }
        let status = command
            .arg(target)
            .status()
            .map_err(|error| error.to_string())?;
        // Explorer returns 1 for successful handoff to an existing shell.
        if status.success() || (cfg!(windows) && !browse && status.code() == Some(1)) {
            Ok(())
        } else {
            Err(format!("Could not open the mods location ({status})."))
        }
    };
    if browse {
        open(OsStr::new("https://modrinth.com/mods"))
    } else {
        let directory = GameDirectory::discover().map_err(|error| error.to_string())?;
        directory
            .prepare_config()
            .map_err(|error| error.to_string())?;
        let folder = directory
            .prepare_user_mods(version)
            .map_err(|error| error.to_string())?;
        open(folder.as_os_str())
    }
}
