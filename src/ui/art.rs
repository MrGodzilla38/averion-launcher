use iced::{
    ContentFit,
    widget::{Image, image::Handle},
};
use std::sync::OnceLock;

pub struct Art {
    pub card: Handle,
    pub sky: &'static [u8],
    pub sky_handle: Handle,
}

macro_rules! cover {
    ($version:literal) => {{
        static COVER: OnceLock<Art> = OnceLock::new();
        COVER.get_or_init(|| {
            let sky = &include_bytes!(concat!(
                "../../assets/version-covers/",
                $version,
                "-sky.rgba"
            ))[..];
            Art {
                card: Handle::from_rgba(
                    320,
                    180,
                    &include_bytes!(concat!("../../assets/version-covers/", $version, ".rgba"))[..],
                ),
                sky,
                sky_handle: Handle::from_rgba(256, 144, sky),
            }
        })
    }};
}

pub fn version_art(version: &str) -> &'static Art {
    match version {
        "26.3" => cover!("26.3"),
        "26.2" => cover!("26.2"),
        "26.1" => cover!("26.1"),
        "1.21" => cover!("1.21"),
        "1.20" => cover!("1.20"),
        "1.19" => cover!("1.19"),
        "1.18" => cover!("1.18"),
        "1.17" => cover!("1.17"),
        "1.16" => cover!("1.16"),
        _ => unreachable!("unknown bundled cover family"),
    }
}

pub fn icon(index: usize, size: u32) -> Image {
    static ICONS: OnceLock<Vec<Handle>> = OnceLock::new();
    let icons = ICONS.get_or_init(|| {
        include_bytes!("../../assets/icons/navigation.rgba")
            .chunks_exact(24 * 24 * 4)
            .map(|data| Handle::from_rgba(24, 24, data))
            .collect()
    });
    Image::new(icons[index].clone())
        .width(size)
        .height(size)
        .content_fit(ContentFit::Contain)
}
