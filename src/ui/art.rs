pub(crate) mod skin;

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

pub(super) fn profile_portrait(width: u32, skin: Option<&Handle>) -> Image {
    static PORTRAIT: OnceLock<Handle> = OnceLock::new();
    Image::new(
        skin.unwrap_or_else(|| PORTRAIT.get_or_init(skin::fallback_portrait))
            .clone(),
    )
    .width(width)
    .height(width * 5 / 4)
    .content_fit(ContentFit::Contain)
}

const PORTRAIT_WIDTH: u32 = 320;
const PORTRAIT_HEIGHT: u32 = 400;

fn finish_portrait(mut portrait: tiny_skia::Pixmap) -> Handle {
    use tiny_skia::{
        BlendMode, Color, GradientStop, LinearGradient, Paint, Point, RadialGradient, Rect,
        SpreadMode, Transform,
    };

    let transform = Transform::identity();
    let bounds = Rect::from_xywh(0., 0., PORTRAIT_WIDTH as f32, PORTRAIT_HEIGHT as f32).unwrap();
    // Fade the combined torso and arms so seams disappear along with the silhouette.
    let fade = Paint {
        shader: LinearGradient::new(
            Point::from_xy(0., 191.),
            Point::from_xy(0., 286.),
            vec![
                GradientStop::new(0., Color::WHITE),
                GradientStop::new(0.45, Color::from_rgba8(255, 255, 255, 118)),
                GradientStop::new(1., Color::TRANSPARENT),
            ],
            SpreadMode::Pad,
            transform,
        )
        .unwrap(),
        blend_mode: BlendMode::DestinationIn,
        ..Paint::default()
    };
    portrait.fill_rect(bounds, &fade, transform, None);

    let center = Point::from_xy(153., 161.);
    let glow = Paint {
        shader: RadialGradient::new(
            center,
            center,
            149.,
            vec![
                GradientStop::new(0., Color::from_rgba8(75, 173, 210, 107)),
                GradientStop::new(0.64, Color::from_rgba8(63, 143, 181, 35)),
                GradientStop::new(1., Color::TRANSPARENT),
            ],
            SpreadMode::Pad,
            transform,
        )
        .unwrap(),
        blend_mode: BlendMode::DestinationOver,
        ..Paint::default()
    };
    portrait.fill_rect(bounds, &glow, transform, None);

    // Iced image handles expect straight alpha.
    let rgba: Vec<_> = portrait
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect();
    Handle::from_rgba(PORTRAIT_WIDTH, PORTRAIT_HEIGHT, rgba)
}
