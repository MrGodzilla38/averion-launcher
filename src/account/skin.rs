//! Read-only lookup of a Java Edition player's public skin.
use crate::http::{self, Https};
use base64::{Engine, engine::general_purpose::STANDARD};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use tiny_skia::{IntSize, Pixmap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Missing,
    Unavailable,
}

pub(crate) struct Skin {
    pub texture: Pixmap,
    pub slim: bool,
}

#[derive(Deserialize)]
struct Player {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct Profile {
    id: String,
    properties: Vec<Property>,
}

#[derive(Deserialize)]
struct Property {
    name: String,
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Textures {
    profile_id: String,
    textures: TextureEntries,
}

#[derive(Deserialize)]
struct TextureEntries {
    #[serde(rename = "SKIN")]
    skin: Option<Texture>,
}

#[derive(Deserialize)]
struct Texture {
    url: String,
    metadata: Option<TextureMetadata>,
}

#[derive(Deserialize)]
struct TextureMetadata {
    model: String,
}

pub(crate) async fn lookup(https: &Https, name: &str) -> Result<Skin, Error> {
    super::validate_name(name).map_err(|_| Error::Missing)?;
    let url = Url::parse(&format!(
        "https://api.minecraftservices.com/minecraft/profile/lookup/name/{name}"
    ))
    .map_err(|_| Error::Unavailable)?;
    let player: Player = json(&metadata(https, url, 4096).await?)?;
    if !player.name.eq_ignore_ascii_case(name) || !valid_hex(&player.id, 32) {
        return Err(Error::Unavailable);
    }

    let url = Url::parse(&format!(
        "https://sessionserver.mojang.com/session/minecraft/profile/{}",
        player.id
    ))
    .map_err(|_| Error::Unavailable)?;
    let (url, digest, slim) = skin_texture(&metadata(https, url, 16 * 1024).await?, &player.id)?;
    let bytes = metadata(https, url, 64 * 1024).await?;
    if Sha256::digest(&bytes).as_slice() != digest {
        return Err(Error::Unavailable);
    }
    let texture = decode(&bytes)?;
    Ok(Skin {
        slim: slim && texture.height() == 64,
        texture,
    })
}

async fn metadata(https: &Https, url: Url, limit: usize) -> Result<Vec<u8>, Error> {
    https
        .metadata(url, limit)
        .await
        .map_err(|error| match error {
            http::Error::Status(StatusCode::NOT_FOUND | StatusCode::NO_CONTENT) => Error::Missing,
            _ => Error::Unavailable,
        })
}

fn json<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(bytes).map_err(|_| Error::Unavailable)
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn skin_texture(bytes: &[u8], expected_id: &str) -> Result<(Url, [u8; 32], bool), Error> {
    let profile: Profile = json(bytes)?;
    if !profile.id.eq_ignore_ascii_case(expected_id) {
        return Err(Error::Unavailable);
    }
    let property = profile
        .properties
        .into_iter()
        .find(|property| property.name == "textures")
        .ok_or(Error::Missing)?;
    let decoded = STANDARD
        .decode(property.value)
        .map_err(|_| Error::Unavailable)?;
    let textures: Textures = json(&decoded)?;
    if !textures.profile_id.eq_ignore_ascii_case(expected_id) {
        return Err(Error::Unavailable);
    }

    let skin = textures.textures.skin.ok_or(Error::Missing)?;
    let source = Url::parse(&skin.url).map_err(|_| Error::Unavailable)?;
    if !matches!(source.scheme(), "http" | "https")
        || source.host_str() != Some("textures.minecraft.net")
        || source.port().is_some()
        || !source.username().is_empty()
        || source.password().is_some()
        || source.query().is_some()
        || source.fragment().is_some()
    {
        return Err(Error::Unavailable);
    }
    let digest = source
        .path()
        .strip_prefix("/texture/")
        .filter(|value| valid_hex(value, 64))
        .ok_or(Error::Unavailable)?;
    // Mojang still returns HTTP URLs. Fetch only the validated digest over HTTPS.
    let url = Url::parse(&format!("https://textures.minecraft.net/texture/{digest}"))
        .map_err(|_| Error::Unavailable)?;
    let mut hash = [0; 32];
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&digest[index * 2..index * 2 + 2], 16)
            .map_err(|_| Error::Unavailable)?;
    }
    Ok((
        url,
        hash,
        skin.metadata
            .is_some_and(|metadata| metadata.model == "slim"),
    ))
}

fn decode(bytes: &[u8]) -> Result<Pixmap, Error> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: 256 * 1024 });
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let header = decoder.read_header_info().map_err(|_| Error::Unavailable)?;
    if header.width != 64 || !matches!(header.height, 32 | 64) {
        return Err(Error::Unavailable);
    }
    let mut reader = decoder.read_info().map_err(|_| Error::Unavailable)?;
    if reader.info().animation_control.is_some() {
        return Err(Error::Unavailable);
    }
    let mut decoded = vec![0; reader.output_buffer_size().ok_or(Error::Unavailable)?];
    let frame = reader
        .next_frame(&mut decoded)
        .map_err(|_| Error::Unavailable)?;
    use png::ColorType::*;
    let mut premultiplied = Vec::with_capacity(frame.width as usize * frame.height as usize * 4);
    for pixel in decoded[..frame.buffer_size()].chunks_exact(frame.color_type.samples()) {
        let [r, g, b, a] = match (frame.color_type, pixel) {
            (Rgba, [r, g, b, a]) => [*r, *g, *b, *a],
            (Rgb, [r, g, b]) => [*r, *g, *b, 255],
            (GrayscaleAlpha, [gray, a]) => [*gray, *gray, *gray, *a],
            (Grayscale, [gray]) => [*gray, *gray, *gray, 255],
            _ => return Err(Error::Unavailable),
        };
        premultiplied
            .extend([r, g, b].map(|ink| ((u16::from(ink) * u16::from(a) + 127) / 255) as u8));
        premultiplied.push(a);
    }
    let size = IntSize::from_wh(frame.width, frame.height).ok_or(Error::Unavailable)?;
    Pixmap::from_vec(premultiplied, size).ok_or(Error::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "853c80ef3c3749fdaa49938b674adae6";
    const HASH: &str = "7fd9ba42a7c81eeea22f1524271ae85a8e045ce0af5a6ae16c6406ae917e68b5";

    fn profile(texture_id: &str, texture: serde_json::Value) -> Vec<u8> {
        let value = STANDARD.encode(
            serde_json::json!({ "profileId": texture_id, "textures": texture }).to_string(),
        );
        serde_json::json!({
            "id": ID, "properties": [{ "name": "textures", "value": value }]
        })
        .to_string()
        .into_bytes()
    }

    #[test]
    fn uses_only_the_selected_players_mojang_texture() {
        let texture = |host| {
            serde_json::json!({
                "SKIN": {
                    "url": format!("http://{host}/texture/{HASH}"),
                    "metadata": { "model": "slim" }
                }
            })
        };
        let (url, digest, slim) =
            skin_texture(&profile(ID, texture("textures.minecraft.net")), ID).unwrap();
        assert_eq!(
            url.as_str(),
            format!("https://textures.minecraft.net/texture/{HASH}")
        );
        assert_eq!(&digest[..3], &[0x7f, 0xd9, 0xba]);
        assert!(slim);
        assert_eq!(
            skin_texture(&profile(ID, texture("example.org")), ID),
            Err(Error::Unavailable)
        );
        assert_eq!(
            skin_texture(&profile("other", texture("textures.minecraft.net")), ID),
            Err(Error::Unavailable)
        );
        assert_eq!(
            skin_texture(&profile(ID, serde_json::json!({})), ID),
            Err(Error::Missing)
        );
    }

    #[test]
    fn decodes_java_skin_sizes_and_preserves_overlay_alpha() {
        for (width, height) in [(64, 32), (64, 64), (65, 64)] {
            let mut bytes = Vec::new();
            let mut encoder = png::Encoder::new(&mut bytes, width, height);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_color(png::ColorType::Rgba);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 128, 0, 128].repeat((width * height) as usize))
                .unwrap();
            writer.finish().unwrap();
            if width == 64 {
                let skin = decode(&bytes).unwrap();
                assert_eq!((skin.width(), skin.height()), (width, height));
                assert_eq!(&skin.data()[..4], &[128, 64, 0, 128]);
            } else {
                assert!(matches!(decode(&bytes), Err(Error::Unavailable)));
            }
        }
    }
}
