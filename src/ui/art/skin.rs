use super::{Handle, PORTRAIT_HEIGHT, PORTRAIT_WIDTH, finish_portrait};
use crate::account::skin::Skin;
use tiny_skia::{IntRect, Pixmap, PixmapPaint, Transform};

#[derive(Clone, Debug)]
pub struct Portraits {
    pub(crate) profile: Handle,
    pub(crate) navigation: Handle,
}

#[derive(Clone, Copy)]
enum Facing {
    Right,
    Left,
}

pub(super) fn fallback_portraits() -> Portraits {
    let mut texture = Pixmap::new(64, 32).unwrap();
    texture.fill(tiny_skia::Color::from_rgba8(66, 122, 150, 255));
    portraits(&Skin {
        texture,
        slim: false,
    })
}

pub(crate) fn portraits(skin: &Skin) -> Portraits {
    Portraits {
        profile: portrait(skin, Facing::Right),
        navigation: portrait(skin, Facing::Left),
    }
}

fn portrait(skin: &Skin, facing: Facing) -> Handle {
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
    let left_arm = Part {
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
    let right_arm = Part {
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
    let left_arm = (left_arm, modern.then_some((48, 48)));
    let right_arm = (right_arm, modern.then_some((40, 32)));
    let (far_arm, near_arm) = match facing {
        Facing::Right => (left_arm, right_arm),
        Facing::Left => (right_arm, left_arm),
    };
    for (part, clothing) in [
        far_arm,
        (torso, modern.then_some((16, 32))),
        near_arm,
        (head, has_hat.then_some((32, 0))),
    ] {
        part.draw(&mut portrait, &skin.texture, clothing, facing);
    }
    let portrait = finish_portrait(portrait);
    match facing {
        Facing::Right => super::portrait_handle(&portrait),
        Facing::Left => {
            let bounds = IntRect::from_xywh(45, 44, 230, 246).unwrap();
            super::portrait_handle(&portrait.clone_rect(bounds).unwrap())
        }
    }
}

struct Part {
    origin: [f32; 3],
    size: [u32; 3],
    uv: (u32, u32),
    mirrored: bool,
}

impl Part {
    fn draw(
        &self,
        portrait: &mut Pixmap,
        atlas: &Pixmap,
        overlay: Option<(u32, u32)>,
        facing: Facing,
    ) {
        let [x, y, z] = self.origin;
        let [width, height, depth] = self.size;
        let (tilt, depth_x) = match facing {
            Facing::Right => (-1.8, -5.),
            Facing::Left => (1.8, 5.),
        };
        let project = |x, y, z| {
            (
                160. + 12. * x + depth_x * z,
                72. + 12. * y + tilt * x - 3. * z,
            )
        };
        let (front_x, front_y) = project(x, y, z);
        let (back_x, back_y) = project(x, y, z + depth as f32);
        // Opposite views expose opposite atlas faces while the front keeps its texture orientation.
        let (side_u, side_transform) = match facing {
            Facing::Right => (
                if self.mirrored { depth + width } else { 0 },
                Transform::from_row(5., 3., 0., 12., back_x, back_y),
            ),
            Facing::Left => {
                let (side_x, side_y) = project(x + width as f32, y, z);
                (
                    if self.mirrored { 0 } else { depth + width },
                    Transform::from_row(5., -3., 0., 12., side_x, side_y),
                )
            }
        };
        let faces = [
            (
                depth,
                0,
                width,
                depth,
                Transform::from_row(12., tilt, -depth_x, 3., back_x, back_y),
                255,
            ),
            (side_u, depth, depth, height, side_transform, 176),
            (
                depth,
                depth,
                width,
                height,
                Transform::from_row(12., tilt, 0., 12., front_x, front_y),
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
        let Handle::Rgba { pixels, .. } = portrait(
            &Skin {
                texture,
                slim: false,
            },
            Facing::Right,
        ) else {
            panic!("expected an RGBA portrait");
        };
        let pixel = |x: usize, y: usize| {
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
    fn draws_the_navigation_portrait_with_the_opposite_side_and_unmirrored_face() {
        let mut texture = Pixmap::new(64, 64).unwrap();
        for y in 8..16 {
            for x in 8..24 {
                let color = match x {
                    8..12 => [255, 0, 0, 255],
                    12..16 => [0, 255, 0, 255],
                    _ => [0, 0, 255, 255],
                };
                let index = (y * 64 + x) * 4;
                texture.data_mut()[index..index + 4].copy_from_slice(&color);
            }
        }
        let Handle::Rgba { pixels, width, .. } = portraits(&Skin {
            texture,
            slim: false,
        })
        .navigation
        else {
            panic!("expected an RGBA portrait");
        };
        let pixel = |x: usize, y: usize| {
            let offset = ((y - 44) * width as usize + (x - 45)) * 4;
            &pixels[offset..offset + 4]
        };
        assert_eq!(pixel(110, 115), &[228, 0, 0, 255]);
        assert_eq!(pixel(165, 115), &[0, 228, 0, 255]);
        assert_eq!(pixel(205, 115), &[0, 0, 176, 255]);
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
        let Handle::Rgba { pixels, .. } = portrait(
            &Skin {
                texture,
                slim: false,
            },
            Facing::Right,
        ) else {
            panic!("expected an RGBA portrait");
        };
        let index = (115 * PORTRAIT_WIDTH as usize + 155) * 4;
        assert!(pixels[index] > 200);
    }
}
