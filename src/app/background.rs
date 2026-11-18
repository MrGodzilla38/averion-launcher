use crate::{theme::BASE, ui::art::version_art};
use iced::{Size, Task, widget::image::Handle};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, PixmapRef, Transform};

use crate::message::Message;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    family: &'static str,
    width: u32,
    height: u32,
}

#[derive(Default)]
pub struct Background {
    pub size: Size,
    pub scale: Option<f32>,
    cached: Option<(Key, Handle)>,
    pending: Option<Key>,
}

impl Background {
    fn key(&self, family: &'static str) -> Option<Key> {
        let scale = self.scale?;
        let width = (self.size.width * scale).round() as u32;
        let height = (self.size.height * scale).round() as u32;

        // Keep a single native-size copy up to 4K. Larger windows use the source.
        (width > 0 && height > 0 && u64::from(width) * u64::from(height) <= 3840 * 2160).then_some(
            Key {
                family,
                width,
                height,
            },
        )
    }

    pub fn handle(&self, family: &'static str) -> Option<Handle> {
        let key = self.key(family)?;
        self.cached
            .as_ref()
            .filter(|(cached, _)| {
                *cached == key
                    || (self.pending.is_some()
                        && cached.width == key.width
                        && cached.height == key.height)
            })
            .map(|(_, handle)| handle.clone())
    }

    pub fn refresh(&mut self, family: &'static str) -> Task<Message> {
        let key = self.key(family);
        if self.cached.as_ref().map(|(cached, _)| *cached) == key || self.pending.is_some() {
            return Task::none();
        }
        let Some(key) = key else {
            self.cached = None;
            return Task::none();
        };
        self.pending = Some(key);
        Task::perform(async move { resize_cover(key) }, move |result| {
            Message::BackgroundReady(key, result)
        })
    }

    pub fn complete(
        &mut self,
        key: Key,
        handle: Option<Handle>,
        family: &'static str,
    ) -> Task<Message> {
        self.pending = None;
        if self.key(family) == Some(key) {
            // Keep using the source if allocation failed.
            self.cached = handle.map(|handle| (key, handle));
            Task::none()
        } else {
            self.refresh(family)
        }
    }
}

fn resize_cover(key: Key) -> Option<Handle> {
    // Composite the source over the window canvas once, before upscaling.
    let base = BASE.into_rgba8();
    let mut pixels = version_art(key.family).sky.to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for (channel, base) in pixel[..3].iter_mut().zip(base) {
            *channel =
                ((u16::from(*channel) * alpha + u16::from(base) * (255 - alpha) + 127) / 255) as u8;
        }
        pixel[3] = 255;
    }
    let source = PixmapRef::from_bytes(&pixels, 256, 144)?;
    let mut target = Pixmap::new(key.width, key.height)?;
    let scale = (key.width as f32 / 256.).max(key.height as f32 / 144.);
    let x = (key.width as f32 - 256. * scale) / 2.;
    let y = (key.height as f32 - 144. * scale) / 2.;
    target.draw_pixmap(
        0,
        0,
        source,
        &PixmapPaint {
            quality: FilterQuality::Bilinear,
            ..Default::default()
        },
        Transform::from_scale(scale, scale).post_translate(x, y),
        None,
    );
    Some(Handle::from_rgba(key.width, key.height, target.take()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cover_fills_the_target_and_stale_results_are_hidden() {
        let mut background = Background {
            size: Size::new(96., 64.),
            scale: Some(1.5),
            ..Background::default()
        };
        let key = background.key("1.20").unwrap();
        let handle = resize_cover(key).unwrap();
        let Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = &handle
        else {
            panic!("expected an RGBA background");
        };
        assert_eq!((*width, *height), (144, 96));
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[3] == 255)
        );
        let _ = background.complete(key, Some(handle), "1.20");
        assert!(background.handle("1.20").is_some());
        assert!(background.handle("1.21").is_none());
        background.scale = Some(2.);
        assert!(background.handle("1.20").is_none());
    }
}
