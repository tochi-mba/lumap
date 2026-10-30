//! Is the camera framed well enough to scan? From two photos (the projector white, then black)
//! it finds where the projector's light lands in the camera's view and says what to fix: the
//! camera cannot see it, it runs off an edge of the picture, it is tiny, the room drowns it out,
//! or something stands in it.

use crate::image::Gray;

/// A side of the camera's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Left edge.
    Left,
    /// Right edge.
    Right,
    /// Top edge.
    Top,
    /// Bottom edge.
    Bottom,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

/// Something about the camera's view that will spoil a scan.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// No projector light reaches the camera's picture.
    Blind,
    /// The projection runs off these edges of the picture.
    CutOff(Vec<Side>),
    /// The projection covers this little of the picture.
    Small(f64),
    /// The projector barely brightens the wall (a bright room, a weak projector).
    Dim,
    /// Part of the projection is hidden, mostly around this area of it.
    Blocked(&'static str),
}

impl Problem {
    /// What to do about it, in a sentence.
    pub fn advice(&self) -> String {
        match self {
            Self::Blind => "The camera can't see the projector's light. Point the camera at the \
                            wall where the projector shines."
                .into(),
            Self::CutOff(sides) => {
                let names: Vec<&str> = sides.iter().map(|s| s.word()).collect();
                format!(
                    "The projection runs off the {} of the camera's view. Turn the camera \
                     towards it, or move it back so the whole picture fits with a little \
                     space around it.",
                    join(&names)
                )
            }
            Self::Small(f) => format!(
                "The projection fills only {:.0}% of the camera's view. Move the camera closer, \
                 or zoom in, so it fills most of the picture.",
                f * 100.0
            ),
            Self::Dim => "The projector barely brightens the wall in the camera's view. lumap \
                          reads faint stripes too, but for more detail move the camera closer or \
                          turn the projector's brightness up."
                .into(),
            Self::Blocked(area) => format!(
                "Something is blocking the {area} of the projection. Move it, or step out of \
                 the light."
            ),
        }
    }

    /// True when scanning anyway would surely fail (rather than lose a little).
    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::Blind)
    }
}

fn join(words: &[&str]) -> String {
    match words {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What the check found.
#[derive(Debug, Clone, PartialEq)]
pub struct Framing {
    /// Problems, worst first; empty when the view is good.
    pub problems: Vec<Problem>,
    /// Where the projector's light lands: 255 inside, 0 outside, camera resolution.
    pub footprint: Gray,
    /// Its bounds in camera pixels: left, top, right, bottom (empty when blind).
    pub bounds: Option<[u32; 4]>,
    /// Share of the camera's picture it covers.
    pub coverage: f64,
    /// How bright the room is next to the projector's own light, inside the projection: the
    /// typical brightness with the projector black over its typical brightening when white.
    /// 0 is a dark room; around 1 or more, room light rivals the projector.
    pub room_light: f64,
}

impl Framing {
    /// How the room's light compares with the projector's, for the scan sheet to say.
    pub fn light(&self) -> Light {
        if self.room_light >= BRIGHT_ROOM {
            Light::Bright
        } else if self.room_light >= LIT_ROOM {
            Light::Lit
        } else {
            Light::Dark
        }
    }
}

/// The room's light next to the projector's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    /// The projector's light is most of what the camera sees.
    Dark,
    /// Room light is there, but the projector clearly outshines it.
    Lit,
    /// Room light rivals the projector.
    Bright,
}

/// `room_light` from which a room counts as lit.
const LIT_ROOM: f64 = 0.25;
/// `room_light` from which room light rivals the projector.
const BRIGHT_ROOM: f64 = 1.0;

/// Share of the picture's width or height counted as touching an edge.
const EDGE_BAND: f64 = 0.015;
/// Share of the projection's side that must touch an edge to count as cut off.
const EDGE_TOUCH: f64 = 0.15;
/// Smaller than this share of the picture is too small to scan well.
const SMALL: f64 = 0.04;
/// Typical white-minus-black brightness below which the projector is drowned out.
const DIM: u8 = 28;
/// Share of the projection's outline that may be dark inside before it counts as blocked.
const HOLES: f64 = 0.06;

/// Checks a camera's framing from its photos of the projector white and black.
pub fn check(white: &Gray, black: &Gray) -> Framing {
    let (w, h) = (white.width, white.height);
    let empty = Framing {
        problems: vec![Problem::Blind],
        footprint: Gray::new(w, h),
        bounds: None,
        coverage: 0.0,
        room_light: 0.0,
    };
    if w == 0 || h == 0 || black.width != w || black.height != h {
        return empty;
    }
    let diff: Vec<u8> = white
        .data
        .iter()
        .zip(&black.data)
        .map(|(a, b)| a.saturating_sub(*b))
        .collect();
    let mut sorted = diff.clone();
    sorted.sort_unstable();
    let p99 = sorted[sorted.len() * 99 / 100];
    if p99 < 15 {
        return empty;
    }
    let threshold = (f64::from(p99) * 0.35).max(12.0) as u8;
    let mut footprint = Gray::new(w, h);
    let mut lit = 0usize;
    for (i, d) in diff.iter().enumerate() {
        if *d >= threshold {
            footprint.data[i] = 255;
            lit += 1;
        }
    }
    let footprint = clean(&footprint);
    let lit_now = footprint.data.iter().filter(|&&v| v > 0).count();
    let coverage = lit_now as f64 / (w as usize * h as usize) as f64;
    if lit_now < (w as usize * h as usize) / 500 || lit == 0 {
        return Framing { footprint, ..empty };
    }
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for y in 0..h {
        for x in 0..w {
            if footprint.get(x, y) > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    let mut problems = Vec::new();
    // Cut off: a long stretch of the footprint lies against an edge of the picture.
    let band_x = ((f64::from(w) * EDGE_BAND).ceil() as u32).max(1);
    let band_y = ((f64::from(h) * EDGE_BAND).ceil() as u32).max(1);
    let touching = |xs: std::ops::Range<u32>, ys: std::ops::Range<u32>, along_x: bool| {
        let mut hits = std::collections::BTreeSet::new();
        for y in ys.clone() {
            for x in xs.clone() {
                if footprint.get(x, y) > 0 {
                    hits.insert(if along_x { x } else { y });
                }
            }
        }
        hits.len()
    };
    let span_x = (x1 - x0 + 1) as f64;
    let span_y = (y1 - y0 + 1) as f64;
    let mut cut = Vec::new();
    if touching(0..band_x, 0..h, false) as f64 > span_y * EDGE_TOUCH {
        cut.push(Side::Left);
    }
    if touching(w - band_x..w, 0..h, false) as f64 > span_y * EDGE_TOUCH {
        cut.push(Side::Right);
    }
    if touching(0..w, 0..band_y, true) as f64 > span_x * EDGE_TOUCH {
        cut.push(Side::Top);
    }
    if touching(0..w, h - band_y..h, true) as f64 > span_x * EDGE_TOUCH {
        cut.push(Side::Bottom);
    }
    if !cut.is_empty() {
        problems.push(Problem::CutOff(cut));
    }
    // Blocked: dark gaps inside the projection, row by row. (Something covering a corner
    // reads as a smaller projection; telling the two apart needs the projector's shape.)
    let mut inside = 0usize;
    let mut holes = [0usize; 9];
    for y in y0..=y1 {
        let row: Vec<u32> = (x0..=x1).filter(|&x| footprint.get(x, y) > 0).collect();
        let (Some(&first), Some(&last)) = (row.first(), row.last()) else {
            continue;
        };
        for x in first..=last {
            inside += 1;
            if footprint.get(x, y) == 0 {
                let gx = ((x - x0) * 3 / (x1 - x0 + 1)).min(2) as usize;
                let gy = ((y - y0) * 3 / (y1 - y0 + 1)).min(2) as usize;
                holes[gy * 3 + gx] += 1;
            }
        }
    }
    let hidden: usize = holes.iter().sum();
    if inside > 0 && hidden as f64 / inside as f64 > HOLES {
        let worst = holes
            .iter()
            .enumerate()
            .max_by_key(|(_, n)| **n)
            .map_or(4, |(i, _)| i);
        let area = [
            "top left",
            "top",
            "top right",
            "left side",
            "middle",
            "right side",
            "bottom left",
            "bottom",
            "bottom right",
        ][worst];
        problems.push(Problem::Blocked(area));
    }
    if coverage < SMALL {
        problems.push(Problem::Small(coverage));
    }
    // Dim: the typical brightening inside the footprint.
    let mut inside_diff: Vec<u8> = diff
        .iter()
        .zip(&footprint.data)
        .filter(|(_, f)| **f > 0)
        .map(|(d, _)| *d)
        .collect();
    inside_diff.sort_unstable();
    let brightening = inside_diff.get(inside_diff.len() / 2).copied();
    if brightening.is_some_and(|m| m < DIM) {
        problems.push(Problem::Dim);
    }
    let mut inside_black: Vec<u8> = black
        .data
        .iter()
        .zip(&footprint.data)
        .filter(|(_, f)| **f > 0)
        .map(|(b, _)| *b)
        .collect();
    inside_black.sort_unstable();
    let room = inside_black
        .get(inside_black.len() / 2)
        .copied()
        .unwrap_or(0);
    let room_light = f64::from(room) / f64::from(brightening.unwrap_or(0).max(1));
    Framing {
        problems,
        footprint,
        bounds: Some([x0, y0, x1, y1]),
        coverage,
        room_light,
    }
}

/// Removes specks (lit pixels with few lit neighbours) and fills pinholes.
fn clean(mask: &Gray) -> Gray {
    let count = |x: u32, y: u32| {
        let mut n = 0;
        for dy in -1i64..=1 {
            for dx in -1i64..=1 {
                if mask.at(i64::from(x) + dx, i64::from(y) + dy) > 0 {
                    n += 1;
                }
            }
        }
        n
    };
    Gray::from_fn(mask.width, mask.height, |x, y| {
        let n = count(x, y);
        if (mask.get(x, y) > 0 && n >= 4) || n >= 7 {
            255
        } else {
            0
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A camera picture with the projector lighting `rect` (left, top, right, bottom), and a
    /// dark patch over `hole` when given.
    fn scene(rect: [u32; 4], hole: Option<[u32; 4]>, lift: u8) -> (Gray, Gray) {
        let inside = |r: [u32; 4], x: u32, y: u32| x >= r[0] && x < r[2] && y >= r[1] && y < r[3];
        let white = Gray::from_fn(160, 90, |x, y| {
            let blocked = hole.is_some_and(|h| inside(h, x, y));
            if inside(rect, x, y) && !blocked {
                20 + lift
            } else {
                20
            }
        });
        (white, Gray::from_fn(160, 90, |_, _| 20))
    }

    #[test]
    fn a_good_view_has_no_problems() {
        let (w, b) = scene([30, 15, 130, 75], None, 150);
        let f = check(&w, &b);
        assert!(f.problems.is_empty(), "{:?}", f.problems);
        assert_eq!(f.bounds, Some([30, 15, 129, 74]));
        assert!(f.coverage > 0.3);
    }

    #[test]
    fn each_problem_is_found_and_explained() {
        let (w, b) = scene([0, 0, 0, 0], None, 0);
        assert_eq!(check(&w, &b).problems, vec![Problem::Blind]);
        let (w, b) = scene([0, 20, 100, 70], None, 150);
        assert!(matches!(&check(&w, &b).problems[0], Problem::CutOff(s) if s == &vec![Side::Left]));
        let (w, b) = scene([60, 0, 160, 90], None, 150);
        assert!(
            matches!(&check(&w, &b).problems[0], Problem::CutOff(s) if s.len() == 3),
            "right, top and bottom"
        );
        let (w, b) = scene([70, 40, 85, 50], None, 150);
        assert!(check(&w, &b)
            .problems
            .iter()
            .any(|p| matches!(p, Problem::Small(_))));
        let (w, b) = scene([30, 15, 130, 75], Some([60, 30, 90, 55]), 150);
        assert_eq!(check(&w, &b).problems, vec![Problem::Blocked("middle")]);
        let (w, b) = scene([30, 15, 130, 75], None, 22);
        assert_eq!(check(&w, &b).problems, vec![Problem::Dim]);
        for p in [
            Problem::Blind,
            Problem::CutOff(vec![Side::Top, Side::Right]),
            Problem::Small(0.02),
            Problem::Dim,
            Problem::Blocked("top"),
        ] {
            assert!(!p.advice().is_empty());
        }
        assert!(Problem::CutOff(vec![Side::Top, Side::Right])
            .advice()
            .contains("top and right"));
        assert!(Problem::Blind.is_blocking() && !Problem::Dim.is_blocking());
        let mismatched = check(&Gray::new(4, 4), &Gray::new(2, 2));
        assert_eq!(mismatched.problems, vec![Problem::Blind]);
    }

    #[test]
    fn room_light_is_measured_against_the_projector() {
        let footprint = |v: u8| {
            Gray::from_fn(100, 60, |x, y| {
                if (20..80).contains(&x) && (15..45).contains(&y) {
                    v
                } else {
                    0
                }
            })
        };
        // Dark room: black is black, white brightens a lot.
        let dark = check(&footprint(200), &Gray::new(100, 60));
        assert_eq!(dark.light(), Light::Dark);
        // Lights on: the wall is already bright with the projector black.
        let lit_black = Gray::from_fn(100, 60, |_, _| 90);
        let lit_white = Gray::from_fn(100, 60, |x, y| {
            if (20..80).contains(&x) && (15..45).contains(&y) {
                160
            } else {
                90
            }
        });
        let lit = check(&lit_white, &lit_black);
        assert!(lit.room_light > 1.0, "{}", lit.room_light);
        assert_eq!(lit.light(), Light::Bright);
        assert_eq!(
            check(&Gray::new(100, 60), &Gray::new(100, 60)).room_light,
            0.0
        );
    }
}
