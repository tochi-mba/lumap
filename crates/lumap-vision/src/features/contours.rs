//! Connected regions and their outer boundaries.

use crate::image::Gray;
use lumap_core::geom::Vec2;

/// The eight neighbour steps, clockwise on screen (y points down) starting east.
const DIRS: [(i64, i64); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// The outer boundary of every 8-connected region of non-zero pixels in `binary`.
///
/// Each boundary is a closed chain of boundary pixel centres (`x + 0.5, y + 0.5`), clockwise on
/// screen (a positive shoelace sum with y pointing down), starting at the region's top-left
/// pixel and not repeating it at the end. Regions come in raster order of their first pixel;
/// holes are not traced, and a lone pixel gives a one-point boundary (Moore-neighbour tracing
/// with Jacob's stopping rule).
pub fn contours(binary: &Gray) -> Vec<Vec<Vec2>> {
    if !binary.is_consistent() {
        return Vec::new();
    }
    let (w, h) = (binary.width as usize, binary.height as usize);
    let (labels, count) = label_regions(w, h, true, |i| u32::from(binary.data[i] != 0));
    let mut firsts = vec![usize::MAX; count as usize + 1];
    for (i, &l) in labels.iter().enumerate() {
        if l != 0 && firsts[l as usize] == usize::MAX {
            firsts[l as usize] = i;
        }
    }
    firsts
        .iter()
        .enumerate()
        .skip(1)
        .map(|(l, &first)| {
            let chain = trace(w, h, first, |i| labels[i] as usize == l);
            centres(w, &chain)
        })
        .collect()
}

/// Labels the connected regions of pixels sharing the same non-zero `key` (4- or 8-connected),
/// numbered from 1 in raster order of their first pixel; key 0 is background and stays 0.
/// Returns the label per pixel and the number of regions.
pub(crate) fn label_regions(
    w: usize,
    h: usize,
    eight: bool,
    key: impl Fn(usize) -> u32,
) -> (Vec<u32>, u32) {
    let mut labels = vec![0u32; w * h];
    let mut next = 0u32;
    let mut stack = Vec::new();
    let steps: &[(i64, i64)] = if eight {
        &DIRS
    } else {
        &[(1, 0), (0, 1), (-1, 0), (0, -1)]
    };
    for start in 0..w * h {
        let k = key(start);
        if k == 0 || labels[start] != 0 {
            continue;
        }
        next += 1;
        labels[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for &(dx, dy) in steps {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if labels[j] == 0 && key(j) == k {
                    labels[j] = next;
                    stack.push(j);
                }
            }
        }
    }
    (labels, next)
}

/// Index into [`DIRS`] of a 4-neighbour step.
fn dir_index(dx: i64, dy: i64) -> usize {
    match (dx, dy) {
        (1, 0) => 0,
        (0, 1) => 2,
        (0, -1) => 6,
        _ => 4,
    }
}

/// Moore-neighbour trace of the outer boundary of the 8-connected region containing `start`,
/// which must be the region's first pixel in raster order (so its west neighbour is outside).
/// `inside` says which pixels belong to the region. Returns pixel indices, clockwise on screen.
pub(crate) fn trace(
    w: usize,
    h: usize,
    start: usize,
    inside: impl Fn(usize) -> bool,
) -> Vec<usize> {
    let at = |x: i64, y: i64| {
        (x >= 0 && y >= 0 && x < w as i64 && y < h as i64).then(|| y as usize * w + x as usize)
    };
    let (sx, sy) = ((start % w) as i64, (start / w) as i64);
    let mut out = vec![start];
    let (mut cx, mut cy) = (sx, sy);
    let mut back = 4usize;
    let mut first_step: Option<usize> = None;
    // Every boundary pixel is entered at most four times; the cap only guards against misuse.
    for _ in 0..4 * w * h + 8 {
        let mut found = None;
        for k in 1..=8 {
            let d = (back + k) % 8;
            let (nx, ny) = (cx + DIRS[d].0, cy + DIRS[d].1);
            if let Some(j) = at(nx, ny).filter(|&j| inside(j)) {
                found = Some((d, nx, ny, j));
                break;
            }
        }
        let Some((d, nx, ny, j)) = found else {
            break;
        };
        if (cx, cy) == (sx, sy) {
            match first_step {
                None => first_step = Some(j),
                Some(f) if f == j => break,
                Some(_) => {}
            }
        }
        let prev = (d + 7) % 8;
        let (bx, by) = (cx + DIRS[prev].0, cy + DIRS[prev].1);
        back = dir_index(bx - nx, by - ny);
        (cx, cy) = (nx, ny);
        out.push(j);
    }
    if out.len() > 1 && out.last() == Some(&start) {
        out.pop();
    }
    out
}

/// Pixel centres of pixel indices.
pub(crate) fn centres(w: usize, chain: &[usize]) -> Vec<Vec2> {
    chain
        .iter()
        .map(|&i| Vec2::new((i % w) as f64 + 0.5, (i / w) as f64 + 0.5))
        .collect()
}
