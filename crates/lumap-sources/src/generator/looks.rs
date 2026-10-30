//! The second batch of generators: the ones the built-in looks are made of. Edge effects for
//! mapped boxes and frames, façade effects, nature and party. Every one is a pure function of
//! `t` at a given size so the oracle can test it, and keeps its per-frame setup (drop points,
//! stars, blocks) outside the per-pixel closure with no allocation inside it.

use super::{hash, value_noise};
use crate::Frame;
use lumap_core::project::GeneratorKind;
use std::f64::consts::{PI, TAU};

/// Renders one of the look generators, or `None` for a kind this module does not draw.
/// `tint` maps a brightness in `0..=1` to the tinted pixel.
// One short, self-contained arm per generator; splitting them up would only scatter them.
#[allow(clippy::too_many_lines)]
pub(super) fn render(
    kind: GeneratorKind,
    t: f64,
    n: u32,
    tint: &dyn Fn(f64) -> [u8; 4],
) -> Option<Frame> {
    let s = f64::from(n);
    let uv = |x: u32, y: u32| ((f64::from(x) + 0.5) / s, (f64::from(y) + 0.5) / s);
    let frame = match kind {
        GeneratorKind::EdgeChase => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let edge = u.min(v).min(1.0 - u).min(1.0 - v);
            if edge > 0.06 {
                return tint(0.0);
            }
            // Position along the perimeter, 0..4, clockwise from the top-left corner.
            let along = if v <= u && v <= 1.0 - u {
                u
            } else if u >= 1.0 - v && u >= v {
                1.0 + v
            } else if v >= u {
                2.0 + (1.0 - u)
            } else {
                3.0 + (1.0 - v)
            };
            let head = (t * 1.2).rem_euclid(4.0);
            let behind = (head - along).rem_euclid(4.0);
            let glow = (1.0 - behind / 1.2).clamp(0.0, 1.0).powi(2);
            tint(0.08 + 0.92 * glow)
        }),
        GeneratorKind::Outline => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let edge = u.min(v).min(1.0 - u).min(1.0 - v);
            let width = 0.03 + 0.07 * (t * 1.5).sin().mul_add(0.5, 0.5);
            let band = ((width - edge) / 0.015).clamp(0.0, 1.0);
            tint(0.04 + 0.96 * band)
        }),
        GeneratorKind::Scan => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let phase = (t * 0.5).rem_euclid(2.0);
            let (pos, coord) = if phase < 1.0 {
                (phase, u)
            } else {
                (phase - 1.0, v)
            };
            let d = (coord - pos).abs();
            let bar = (1.0 - d / 0.06).clamp(0.0, 1.0).powi(2);
            let trail = if coord < pos {
                (1.0 - (pos - coord) / 0.35).clamp(0.0, 1.0) * 0.25
            } else {
                0.0
            };
            tint(0.03 + bar + trail)
        }),
        GeneratorKind::Bricks => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let rows = 8.0;
            let row = (v * rows).floor();
            let offset = if row as i64 % 2 == 0 { 0.0 } else { 0.125 };
            let col = ((u + offset) * 4.0).floor();
            let (fu, fv) = (((u + offset) * 4.0).fract(), (v * rows).fract());
            let mortar = fu < 0.05 || fv < 0.1;
            let breathe = (t * 1.3 - (col + row * 0.5) * 0.6).sin().mul_add(0.35, 0.6);
            tint(if mortar { 0.03 } else { breathe })
        }),
        GeneratorKind::Windows => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let (cols, rows) = (6.0, 5.0);
            let (col, row) = ((u * cols).floor(), (v * rows).floor());
            let (fu, fv) = ((u * cols).fract(), (v * rows).fract());
            let frame = !(0.15..0.85).contains(&fu) || !(0.15..0.85).contains(&fv);
            if frame {
                return tint(0.02);
            }
            // Each window has its own slow clock; it is lit while its noise is high.
            let clock = t * 0.25 + hash(col as i64, row as i64, 7) * 10.0;
            let lit = value_noise(col * 3.1, row * 2.7, clock);
            let v = ((lit - 0.45) / 0.2).clamp(0.0, 1.0);
            let glass = 0.85 + 0.15 * (fv * PI).sin();
            tint(0.05 + 0.95 * v * glass)
        }),
        GeneratorKind::Blocks => {
            let cols = 10u32;
            let cell = s / f64::from(cols);
            // The stack grows one block every 0.5 s and clears every 20 s; one falling block at a time.
            let cycle = (t * 2.0).rem_euclid(40.0);
            let landed = cycle.floor() as u32;
            let fall = cycle.fract();
            let height_of = |c: u32| {
                (0..landed)
                    .filter(|&i| (hash(i64::from(i), 1, 3) * f64::from(cols)) as u32 == c)
                    .count() as u32
            };
            let falling_col = (hash(i64::from(landed), 1, 3) * f64::from(cols)) as u32;
            let land_row = height_of(falling_col);
            let fall_y = (f64::from(cols) - 1.0 - f64::from(land_row)) * fall;
            Frame::from_fn(n, n, |x, y| {
                let c = (f64::from(x) / cell) as u32;
                let r_from_bottom = ((s - f64::from(y) - 0.5) / cell) as u32;
                let fy = f64::from(y) / cell;
                let in_cell = ((f64::from(x) / cell).fract() > 0.08)
                    && ((f64::from(y) / cell).fract() > 0.08);
                let stacked = r_from_bottom < height_of(c);
                // The falling block keeps its own gap so it reads as one block between rows.
                let into = fy - fall_y;
                let falling = c == falling_col && (0.08..1.0).contains(&into);
                let in_column = (f64::from(x) / cell).fract() > 0.08;
                let shade = 0.55 + 0.45 * hash(i64::from(c), i64::from(r_from_bottom), 5);
                tint(if falling && in_column {
                    1.0
                } else if !in_cell {
                    0.0
                } else if stacked {
                    shade
                } else {
                    0.02
                })
            })
        }
        GeneratorKind::Wireframe => {
            // A unit cube rotated and projected; drawn as distance to its twelve edges.
            let (a, b) = (t * 0.7, t * 0.45);
            let (sa, ca, sb, cb) = (a.sin(), a.cos(), b.sin(), b.cos());
            let project = |p: [f64; 3]| {
                let (x, y, z) = (p[0], p[1], p[2]);
                let (x1, z1) = (x * ca - z * sa, x * sa + z * ca);
                let (y2, z2) = (y * cb - z1 * sb, y * sb + z1 * cb);
                let d = 3.2 + z2;
                (0.5 + 0.62 * x1 / d, 0.5 + 0.62 * y2 / d)
            };
            let corners: [[f64; 3]; 8] = [
                [-1.0, -1.0, -1.0],
                [1.0, -1.0, -1.0],
                [1.0, 1.0, -1.0],
                [-1.0, 1.0, -1.0],
                [-1.0, -1.0, 1.0],
                [1.0, -1.0, 1.0],
                [1.0, 1.0, 1.0],
                [-1.0, 1.0, 1.0],
            ];
            let pts: [(f64, f64); 8] = std::array::from_fn(|i| project(corners[i]));
            let edges: [(usize, usize); 12] = [
                (0, 1),
                (1, 2),
                (2, 3),
                (3, 0),
                (4, 5),
                (5, 6),
                (6, 7),
                (7, 4),
                (0, 4),
                (1, 5),
                (2, 6),
                (3, 7),
            ];
            Frame::from_fn(n, n, |x, y| {
                let (u, v) = uv(x, y);
                let mut best = f64::MAX;
                for (i, j) in edges {
                    let (ax, ay) = pts[i];
                    let (bx, by) = pts[j];
                    let (dx, dy) = (bx - ax, by - ay);
                    let len2 = dx * dx + dy * dy;
                    let tt = if len2 > 0.0 {
                        ((u - ax) * dx + (v - ay) * dy) / len2
                    } else {
                        0.0
                    }
                    .clamp(0.0, 1.0);
                    let (px, py) = (ax + dx * tt, ay + dy * tt);
                    best = best.min(((u - px).powi(2) + (v - py).powi(2)).sqrt());
                }
                tint(0.02 + 0.98 * (1.0 - best / 0.012).clamp(0.0, 1.0))
            })
        }
        GeneratorKind::Water => {
            let drops: [(f64, f64, f64); 6] = std::array::from_fn(|i| {
                let k = i as i64;
                (
                    0.15 + 0.7 * hash(k, 11, 1),
                    0.15 + 0.7 * hash(k, 13, 2),
                    hash(k, 17, 3) * 5.0,
                )
            });
            Frame::from_fn(n, n, |x, y| {
                let (u, v) = uv(x, y);
                let mut h = 0.0;
                for (cx, cy, phase) in drops {
                    let r = ((u - cx).powi(2) + (v - cy).powi(2)).sqrt();
                    h += (r * 40.0 - t * 3.0 - phase).sin() * (1.0 - r).clamp(0.0, 1.0)
                        / (1.0 + r * 6.0);
                }
                tint((h * 0.35 + 0.5).clamp(0.0, 1.0) * 0.9 + 0.05)
            })
        }
        GeneratorKind::Fire => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            // Noise stretched upwards and scrolled up, so it reads as tongues of flame, hottest
            // at the bottom and burning out towards the top.
            let flame = value_noise(u * 5.0, v * 2.5 + t * 1.8, t * 0.3) * 0.7
                + value_noise(u * 11.0, v * 6.0 + t * 3.5, t * 0.6) * 0.3;
            let heat = (flame - (1.0 - v) * 1.2 + 0.45).clamp(0.0, 1.0);
            tint(heat.powf(1.5))
        }),
        GeneratorKind::Aurora => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let curtain = value_noise(u * 3.0 + t * 0.15, v * 1.5, t * 0.1);
            let bands = (u * 20.0 + curtain * 8.0 + t * 0.4).sin().mul_add(0.5, 0.5);
            let fade = (1.0 - (v - 0.45).abs() * 1.6).clamp(0.0, 1.0);
            tint((0.02 + 1.8 * curtain * bands * fade).min(1.0))
        }),
        GeneratorKind::Stars => {
            let stars: [(f64, f64, f64, f64); 120] = std::array::from_fn(|i| {
                let k = i as i64;
                (
                    hash(k, 21, 1),
                    hash(k, 23, 2),
                    0.5 + hash(k, 29, 3) * 3.0,
                    hash(k, 31, 4) * TAU,
                )
            });
            let drift = t * 0.004;
            Frame::from_fn(n, n, |x, y| {
                let (u, v) = uv(x, y);
                let mut best: f64 = 0.0;
                for (sx, sy, rate, phase) in stars {
                    let dx = ((u - sx - drift).rem_euclid(1.0) + 0.5).rem_euclid(1.0) - 0.5;
                    let dy = v - sy;
                    let d2 = dx * dx + dy * dy;
                    if d2 < 0.0002 {
                        let twinkle = (t * rate + phase).sin().mul_add(0.4, 0.6);
                        best = best.max((1.0 - d2 / 0.0002) * twinkle);
                    }
                }
                tint(0.01 + 0.99 * best)
            })
        }
        GeneratorKind::Disco => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let (col, row) = ((u * 8.0).floor() as i64, (v * 8.0).floor() as i64);
            let beat = (t * 2.0).floor() as i64;
            let on = hash(col, row, beat) > 0.55;
            let gap = (u * 8.0).fract() < 0.06 || (v * 8.0).fract() < 0.06;
            let fade = 1.0 - (t * 2.0).fract() * 0.5;
            tint(if gap {
                0.0
            } else if on {
                fade
            } else {
                0.08
            })
        }),
        GeneratorKind::Kaleidoscope => Frame::from_fn(n, n, |x, y| {
            let (u, v) = uv(x, y);
            let (dx, dy) = (u - 0.5, v - 0.5);
            let r = (dx * dx + dy * dy).sqrt();
            let wedge = TAU / 6.0;
            let mut angle = dy.atan2(dx).rem_euclid(wedge);
            if angle > wedge / 2.0 {
                angle = wedge - angle;
            }
            let (px, py) = (r * angle.cos(), r * angle.sin());
            let p = (px * 14.0 + t).sin()
                + (py * 11.0 - t * 1.4).sin()
                + ((px + py) * 9.0 + t * 0.6).sin();
            tint(p.mul_add(1.0 / 6.0, 0.5).clamp(0.0, 1.0) * (1.0 - r).clamp(0.0, 1.0))
        }),
        GeneratorKind::Hearts => {
            // Fourteen hearts rising at their own pace, swaying a little, all beating together.
            let beat = (t * 1.1 * TAU).sin().max(0.0).powi(6) * 0.12;
            let hearts: [(f64, f64, f64); 14] = std::array::from_fn(|i| {
                let k = i as i64;
                let size = 0.045 + hash(k, 41, 1) * 0.05;
                let rise = 0.06 + hash(k, 43, 2) * 0.08;
                let phase = hash(k, 47, 3);
                let y = 1.15 - (t * rise + phase * 1.3).rem_euclid(1.3);
                let x = 0.08 + 0.84 * hash(k, 53, 4) + 0.04 * (t * 0.7 + phase * TAU).sin();
                (x, y, size * (1.0 + beat))
            });
            Frame::from_fn(n, n, |x, y| {
                let (u, v) = uv(x, y);
                let mut best: f64 = 0.0;
                for (cx, cy, size) in hearts {
                    // The heart curve (x² + y² − 1)³ − x²y³ ≤ 0, y pointing up.
                    let hx = (u - cx) / size;
                    let hy = -(v - cy) / size + 0.25;
                    if hx.abs() > 1.6 || hy.abs() > 1.8 {
                        continue;
                    }
                    let f = (hx * hx + hy * hy - 1.0).powi(3) - hx * hx * hy.powi(3);
                    let glow = if f <= 0.0 {
                        (0.75 + (-f / 0.4).min(1.0) * 0.25).min(1.0)
                    } else {
                        (1.0 - f / 0.5).clamp(0.0, 1.0) * 0.35
                    };
                    best = best.max(glow);
                }
                // A faint warm wash so the wall is never fully dark between hearts.
                tint(0.04 + 0.96 * best)
            })
        }
        _ => return None,
    };
    Some(frame)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_look_generators_render_and_the_first_batch_kinds_do_not() {
        let tint = |v: f64| [(v.clamp(0.0, 1.0) * 255.0) as u8, 0, 0, 255];
        for kind in GeneratorKind::ALL {
            let drawn = render(kind, 1.0, 24, &tint);
            assert_eq!(
                drawn.is_some(),
                kind.category() != lumap_core::project::GeneratorCategory::Classic,
                "{kind:?}"
            );
            if let Some(f) = drawn {
                assert_eq!((f.width, f.height), (24, 24));
                assert!(
                    f.rgba.chunks_exact(4).any(|p| p[0] > 40),
                    "{kind:?} draws something"
                );
            }
        }
        // The edge chase lights the border and leaves the middle dark.
        let f = render(GeneratorKind::EdgeChase, 0.3, 64, &tint).unwrap();
        assert!(f.pixel(32, 32)[0] < 10);
        assert!((0..64).any(|x| f.pixel(x, 1)[0] > 200));
    }
}

/// Renders every look generator to `docs/images/looks/<kind>.png` at three moments side by
/// side, for the README, the website and tuning by eye. Ignored by default because it writes
/// into the repository: `cargo test -p lumap-sources -- --ignored render_look_previews`.
#[cfg(test)]
#[test]
#[ignore = "writes docs/images/looks; run on purpose after changing a generator"]
#[allow(clippy::unwrap_used)]
fn render_look_previews() {
    let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/images/looks");
    std::fs::create_dir_all(&out).unwrap();
    let size = 192u32;
    for kind in GeneratorKind::ALL {
        if kind.category() == lumap_core::project::GeneratorCategory::Classic {
            continue;
        }
        // Each preview in the colour of the first look that uses the generator.
        let color = lumap_core::looks::LOOKS
            .iter()
            .find(|l| l.kind == kind)
            .map_or([0.84, 1.0, 0.25, 1.0], |l| l.color);
        let shots: Vec<Frame> = [0.7, 2.3, 4.1]
            .iter()
            .map(|&t| super::render(kind, t, color, size))
            .collect();
        let mut img = image::RgbaImage::new(size * 3 + 8, size);
        for (i, f) in shots.iter().enumerate() {
            for y in 0..size {
                for x in 0..size {
                    let p = f.pixel(i64::from(x), i64::from(y));
                    img.put_pixel(x + i as u32 * (size + 4), y, image::Rgba(p));
                }
            }
        }
        let name = kind.label().to_lowercase().replace(' ', "-");
        img.save(out.join(format!("{name}.png"))).unwrap();
    }
}
