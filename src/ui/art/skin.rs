use super::{Handle, PORTRAIT_HEIGHT, PORTRAIT_WIDTH, finish_portrait};
use crate::account::skin::Skin;
use tiny_skia::{IntRect, Pixmap, PixmapPaint, Transform};

pub(super) fn fallback_portrait() -> Handle {
    let mut texture = Pixmap::new(64, 32).unwrap();
    texture.fill(tiny_skia::Color::from_rgba8(66, 122, 150, 255));
    portrait(&Skin {
        texture,
        slim: false,
    })
}

pub(crate) fn portrait(skin: &Skin) -> Handle {
    let mut portrait = Pixmap::new(PORTRAIT_WIDTH, PORTRAIT_HEIGHT).unwrap();
    let modern = skin.texture.height() == 64;
    // Legacy skins can fill the unused hat area with opaque pixels. Minecraft
    // treats that block as empty unless it contains transparency.
    let has_hat = modern
        || skin
            .texture
            .pixels()
            .as_chunks::<64>()
            .0
            .iter()
            .any(|row| row[32..].iter().any(|pixel| pixel.alpha() < 128));
    let arm = if skin.slim { 3 } else { 4 };
    let far_arm = Part {
        origin: [4., 8., -2.],
        size: [arm, 12, 4],
        uv: if modern { (32, 48) } else { (40, 16) },
        mirrored: !modern,
    };
    let torso = Part {
        origin: [-4., 8., -2.],
        size: [8, 12, 4],
        uv: (16, 16),
        mirrored: false,
    };
    let near_arm = Part {
        origin: [-4. - arm as f32, 8., -2.],
        size: [arm, 12, 4],
        uv: (40, 16),
        mirrored: false,
    };
    let head = Part {
        origin: [-4., 0., -4.],
        size: [8, 8, 8],
        uv: (0, 0),
        mirrored: false,
    };
    for (part, clothing) in [
        (far_arm, modern.then_some((48, 48))),
        (torso, modern.then_some((16, 32))),
        (near_arm, modern.then_some((40, 32))),
        (head, has_hat.then_some((32, 0))),
    ] {
        part.draw(&mut portrait, &skin.texture, clothing);
    }
    finish_portrait(portrait)
}

struct Part {
    origin: [f32; 3],
    size: [u32; 3],
    uv: (u32, u32),
    mirrored: bool,
}

impl Part {
    fn draw(&self, portrait: &mut Pixmap, atlas: &Pixmap, overlay: Option<(u32, u32)>) {
        let [x, y, z] = self.origin;
        let [width, height, depth] = self.size;
        let project = |x, y, z| (160. + 12. * x - 5. * z, 72. + 12. * y - 1.8 * x - 3. * z);
        let (front_x, front_y) = project(x, y, z);
        let (back_x, back_y) = project(x, y, z + depth as f32);
        // The front face sits to the right of the visible side. Keep the
        // atlas orientation independent of the projection and legacy mirroring.
        let faces = [
            (
                depth,
                0,
                width,
                depth,
                Transform::from_row(12., -1.8, 5., 3., back_x, back_y),
                255,
            ),
            (
                if self.mirrored { depth + width } else { 0 },
                depth,
                depth,
                height,
                Transform::from_row(5., 3., 0., 12., back_x, back_y),
                176,
            ),
            (
                depth,
                depth,
                width,
                height,
                Transform::from_row(12., -1.8, 0., 12., front_x, front_y),
                228,
            ),
        ];
        for (u, v, width, height, transform, light) in faces {
            let transform = if self.mirrored {
                Transform::from_scale(-1., 1.)
                    .post_translate(width as f32, 0.)
                    .post_concat(transform)
            } else {
                transform
            };
            for ((u_start, v_start), base) in [(Some(self.uv), true), (overlay, false)]
                .into_iter()
                .filter_map(|(uv, base)| uv.map(|uv| (uv, base)))
            {
                let bounds =
                    IntRect::from_xywh((u_start + u) as i32, (v_start + v) as i32, width, height)
                        .unwrap();
                let mut face = atlas.clone_rect(bounds).unwrap();
                for pixel in face.data_mut().as_chunks_mut::<4>().0 {
                    for ink in &mut pixel[..3] {
                        *ink = (u16::from(*ink) * light / 255) as u8;
                    }
                    if base {
                        pixel[3] = 255;
                    }
                }
                portrait.draw_pixmap(
                    0,
                    0,
                    face.as_ref(),
                    &PixmapPaint::default(),
                    transform,
                    None,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paints_the_skin_front_to_the_right_of_the_side_and_fades_the_body() {
        let mut texture = Pixmap::new(64, 64).unwrap();
        for y in 8..16 {
            for x in 0..16 {
                let index = (y * 64 + x) * 4;
                texture.data_mut()[index..index + 4].copy_from_slice(if x < 8 {
                    &[0, 0, 255, 255]
                } else {
                    &[255, 0, 0, 255]
                });
            }
        }
        let Handle::Rgba { pixels, .. } = portrait(&Skin {
            texture,
            slim: false,
        }) else {
            panic!("expected an RGBA portrait");
        };
        let pixel = |x, y| {
            let offset = (y * PORTRAIT_WIDTH as usize + x) * 4;
            &pixels[offset..offset + 4]
        };
        let side = pixel(110, 115);
        let front = pixel(155, 115);
        assert!(side[2] > side[0] && front[0] > front[2]);
        assert!(pixel(160, 204)[3] > pixel(160, 288)[3]);
        assert_eq!(pixel(160, 332)[3], 0);
    }

    #[test]
    fn opaque_legacy_hat_area_does_not_cover_the_face() {
        let mut texture = Pixmap::new(64, 32).unwrap();
        texture.fill(tiny_skia::Color::BLACK);
        for y in 8..16 {
            for x in 8..16 {
                texture.data_mut()[(y * 64 + x) * 4] = 255;
            }
        }
        let Handle::Rgba { pixels, .. } = portrait(&Skin {
            texture,
            slim: false,
        }) else {
            panic!("expected an RGBA portrait");
        };
        let index = (115 * PORTRAIT_WIDTH as usize + 155) * 4;
        assert!(pixels[index] > 200);
    }
}
