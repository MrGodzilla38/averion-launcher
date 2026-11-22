use crate::{
    account::Session,
    mojang::{
        Error, Platform, Prepared,
        arguments::{Argument, allowed},
    },
};
use std::path::Path;

const SEPARATOR: &str = if cfg!(windows) { ";" } else { ":" };

pub(super) fn resolve(
    game: &Prepared,
    session: &Session,
    platform: Platform,
    crash_log: &Path,
) -> Result<Vec<String>, Error> {
    let classpath = game
        .classpath
        .iter()
        .map(|entry| {
            let value = path(entry)?;
            if value.contains(SEPARATOR) {
                return Err(Error::Invalid(
                    "classpath entry contains the platform separator",
                ));
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(SEPARATOR);
    let libraries = game.game_directory.join("libraries");
    let mut placeholders = [
        ("auth_player_name", session.player_name.as_str()),
        ("auth_uuid", session.uuid.as_str()),
        ("auth_access_token", session.access_token.as_str()),
        ("auth_session", session.access_token.as_str()),
        ("user_type", session.user_type.as_str()),
        ("clientid", session.client_id.as_str()),
        ("auth_xuid", session.xuid.as_str()),
        ("user_properties", "{}"),
        ("profile_properties", "{}"),
        ("version_name", game.id.as_str()),
        ("version_type", game.kind.as_str()),
        ("game_directory", path(&game.game_directory)?),
        ("assets_root", path(&game.assets_root)?),
        ("game_assets", path(&game.assets_root)?),
        ("assets_index_name", game.asset_index_id.as_str()),
        ("natives_directory", path(&game.natives_directory)?),
        ("library_directory", path(&libraries)?),
        ("classpath", classpath.as_str()),
        ("classpath_separator", SEPARATOR),
        ("launcher_name", "Averion"),
        ("launcher_version", env!("CARGO_PKG_VERSION")),
        ("path", ""),
    ];
    let mut args = vec![format!("-XX:ErrorFile={}", path(crash_log)?)];
    if let Some(jvm) = &game.arguments.jvm {
        append(&mut args, jvm, platform, &placeholders)?;
    } else {
        if platform.os == "windows" {
            args.push("-XX:HeapDumpPath=MojangTricksIntelDriversForPerformance_javaw.exe_minecraft.exe.heapdump".into());
        }
        for template in [
            "-Djava.library.path=${natives_directory}",
            "-cp",
            "${classpath}",
        ] {
            args.push(expand(template, &placeholders)?);
        }
    }
    args.extend(game.fabric_jvm.iter().cloned());
    args.push(format!(
        "-Dfabric.modsFolder={}",
        path(&game.mods_directory)?
    ));
    let managed = game
        .managed_mods
        .iter()
        .map(|jar| {
            let jar = path(jar)?;
            if jar.contains(SEPARATOR) {
                return Err(Error::Invalid(
                    "Fabric mod path contains the platform separator",
                ));
            }
            Ok(jar)
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(SEPARATOR);
    args.push(format!("-Dfabric.addMods={managed}"));
    if let Some(logging) = &game.log_config {
        placeholders.last_mut().unwrap().1 = path(&logging.file.path)?;
        args.push(expand(&logging.argument, &placeholders)?);
    }
    args.push(game.main_class.clone());
    if let Some(arguments) = &game.arguments.game {
        append(&mut args, arguments, platform, &placeholders)?;
    } else if let Some(legacy) = &game.legacy {
        for template in legacy.split_whitespace() {
            args.push(expand(template, &placeholders)?);
        }
    }
    Ok(args)
}

fn path(value: &Path) -> Result<&str, Error> {
    value.to_str().ok_or(Error::Invalid(
        "launch path cannot be represented as Unicode",
    ))
}

fn append(
    args: &mut Vec<String>,
    templates: &[Argument],
    platform: Platform,
    placeholders: &[(&str, &str)],
) -> Result<(), Error> {
    for argument in templates {
        let values = match argument {
            Argument::Plain(value) => std::slice::from_ref(value),
            Argument::Conditional(value) if allowed(&value.rules, platform, &[])? => {
                value.value.parts()
            }
            Argument::Conditional(_) => continue,
        };
        for value in values {
            args.push(expand(value, placeholders)?);
        }
    }
    Ok(())
}

fn expand(mut template: &str, placeholders: &[(&str, &str)]) -> Result<String, Error> {
    let mut value = String::with_capacity(template.len());
    while let Some((prefix, tail)) = template.split_once("${") {
        let (key, remaining) = tail
            .split_once('}')
            .ok_or(Error::Invalid("unclosed launch placeholder"))?;
        let replacement = placeholders
            .iter()
            .find_map(|(name, value)| (*name == key).then_some(value))
            .ok_or(Error::Invalid("unknown launch placeholder"))?;
        value.push_str(prefix);
        value.push_str(replacement);
        template = remaining;
    }
    value.push_str(template);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mojang::arguments::Arguments;

    #[test]
    fn resolves_plain_and_conditional_arguments_without_splitting_paths() {
        let spec: Arguments = serde_json::from_str(r#"{"jvm":[
            "-Dtest=${test}", {"rules":[{"action":"allow"},{"action":"disallow","os":{"name":"windows"}}],"value":["-cp","${test}"]},
            {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"${missing}"},
            {"rules":[{"action":"allow","os":{"arch":"x86"}}],"value":"${missing}"},
            {"rules":[{"action":"allow","os":{"arch":"amd64"},"features":{"is_demo_user":false}}],"value":"--64bit"}
        ]}"#).unwrap();
        let mut args = Vec::new();
        append(
            &mut args,
            &spec.jvm.unwrap(),
            Platform { os: "linux" },
            &[("test", "game path/${literal}")],
        )
        .unwrap();
        assert_eq!(
            args,
            [
                "-Dtest=game path/${literal}",
                "-cp",
                "game path/${literal}",
                "--64bit"
            ]
        );
        assert!(expand("${missing}", &[]).is_err());
        assert!(expand("${", &[]).is_err());
    }
}
