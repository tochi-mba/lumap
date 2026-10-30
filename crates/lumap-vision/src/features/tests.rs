#![allow(clippy::float_cmp, clippy::unwrap_used, clippy::expect_used)]

use super::polygon::{boundary_distance, perimeter, segment_distance};
use super::prepared::{sample, Prepared};
use super::scene::{corner_error, paint, scene, Shape};
use super::shapes::{edge_support, is_quad_like, quad_from_outline, refine_circle, refine_polygon};
use super::*;
use crate::image::{Gray, Rgba};
use lumap_core::geom::Vec2;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// A bright 20×14 rectangle covering `[10, 30) × [8, 22)` on a dark 40×30 image.
fn rectangle() -> Gray {
    Gray::from_fn(40, 30, |x, y| {
        if (10..30).contains(&x) && (8..22).contains(&y) {
            200
        } else {
            40
        }
    })
}

fn rect_outline() -> [Vec2; 4] {
    [v(10.0, 8.0), v(30.0, 8.0), v(30.0, 22.0), v(10.0, 22.0)]
}

#[test]
fn sobel_measures_ramps_with_clamped_borders() {
    let ramp = Gray::from_fn(8, 5, |x, _| (x * 10) as u8);
    let (gx, gy) = sobel(&ramp);
    assert_eq!(gx[2 * 8 + 3], 80.0, "8 times the slope");
    assert_eq!(gx[2 * 8], 40.0, "half a step at the clamped border");
    assert!(gy.iter().all(|&g| g == 0.0));
    let (ex, ey) = sobel(&Gray::new(0, 0));
    assert!(ex.is_empty() && ey.is_empty());
    let bad = Gray {
        width: 4,
        height: 4,
        data: vec![1; 3],
    };
    assert_eq!(sobel(&bad).0, vec![0.0; 16]);
}

#[test]
fn blur_spreads_a_dot_symmetrically_and_keeps_flat_images() {
    let flat = Gray::from_fn(9, 7, |_, _| 120);
    assert_eq!(blur(&flat, 1.5), flat);
    let mut dot = Gray::new(11, 11);
    dot.set(5, 5, 255);
    let b = blur(&dot, 1.0);
    assert!((30..255).contains(&b.get(5, 5)), "{}", b.get(5, 5));
    assert_eq!(b.get(4, 5), b.get(6, 5));
    assert_eq!(b.get(5, 4), b.get(5, 6));
    let sum: i64 = b.data.iter().map(|&p| i64::from(p)).sum();
    assert!((sum - 255).abs() <= 12, "{sum}");
    assert_eq!(blur(&dot, 0.0), dot);
    assert_eq!(blur(&dot, f64::NAN), dot);
}

#[test]
fn canny_traces_thin_edges_on_the_boundary_only() {
    let img = blur(&rectangle(), 1.0);
    let e = canny(&img, 20.0, 45.0);
    assert_eq!((e.width, e.height), (40, 30));
    let outline = rect_outline();
    let mut count = 0;
    for y in 0..30 {
        for x in 0..40 {
            if e.get(x, y) != 0 {
                count += 1;
                let p = v(f64::from(x) + 0.5, f64::from(y) + 0.5);
                assert!(boundary_distance(p, &outline) <= 1.5, "stray edge at {p:?}");
            }
        }
    }
    assert!(count >= 60, "{count}");
    // One pixel wide: a row and a column through the middle cross exactly two edges.
    assert_eq!((0..40).filter(|&x| e.get(x, 15) != 0).count(), 2);
    assert_eq!((0..30).filter(|&y| e.get(20, y) != 0).count(), 2);
    // Flat images, tiny images and impossible thresholds give nothing.
    let flat = Gray::from_fn(20, 20, |_, _| 90);
    assert!(canny(&flat, 1.0, 2.0).data.iter().all(|&p| p == 0));
    assert!(canny(&img, 1e9, 1e9).data.iter().all(|&p| p == 0));
    assert_eq!(canny(&Gray::new(2, 2), 1.0, 2.0), Gray::new(2, 2));
    let bad = Gray {
        width: 3,
        height: 3,
        data: vec![],
    };
    assert_eq!(canny(&bad, 1.0, 2.0), Gray::new(3, 3));
    // A high threshold below the low one acts as the low one.
    assert_eq!(canny(&img, 45.0, 10.0), canny(&img, 45.0, 45.0));
}

#[test]
fn corners_find_the_rectangle_corners_to_sub_pixel_precision() {
    let img = blur(&rectangle(), 1.0);
    let found = corners(&img, 10, 0.1, 5.0);
    assert_eq!(found.len(), 4, "{found:?}");
    for truth in rect_outline() {
        let best = found
            .iter()
            .map(|c| c.distance(truth))
            .fold(f64::INFINITY, f64::min);
        assert!(best < 0.6, "corner {truth:?} missed by {best}");
    }
    for (i, a) in found.iter().enumerate() {
        for b in &found[i + 1..] {
            assert!(a.distance(*b) >= 5.0);
        }
    }
    assert_eq!(corners(&img, 2, 0.1, 5.0).len(), 2);
    assert!(corners(&img, 0, 0.1, 5.0).is_empty());
    assert!(corners(&Gray::from_fn(20, 20, |_, _| 9), 5, 0.1, 3.0).is_empty());
    assert!(corners(&Gray::new(4, 4), 5, 0.1, 3.0).is_empty());
    // Without a minimum distance every peak above the quality floor counts.
    assert!(corners(&img, 50, f64::NAN, f64::NAN).len() >= 4);
}

#[test]
fn contours_trace_outer_boundaries_clockwise_in_raster_order() {
    let mut g = Gray::new(12, 10);
    for (x, y) in [(1, 1), (2, 1), (3, 1), (1, 2), (2, 2), (3, 2)] {
        g.set(x, y, 255);
    }
    g.set(8, 2, 255);
    for y in 4..9 {
        for x in 5..10 {
            let hole = (6..9).contains(&x) && (5..8).contains(&y);
            g.set(x, y, if hole { 0 } else { 1 });
        }
    }
    let cs = contours(&g);
    assert_eq!(cs.len(), 3);
    assert_eq!(
        cs[0],
        vec![
            v(1.5, 1.5),
            v(2.5, 1.5),
            v(3.5, 1.5),
            v(3.5, 2.5),
            v(2.5, 2.5),
            v(1.5, 2.5)
        ]
    );
    assert!(signed_area2(&cs[0]) > 0.0, "clockwise on screen");
    assert_eq!(cs[1], vec![v(8.5, 2.5)]);
    assert_eq!(cs[2].len(), 16, "the ring's hole is not traced");
    assert_eq!(cs[2][0], v(5.5, 4.5));
    // A one-pixel-wide line is walked out and back.
    let mut line = Gray::new(8, 8);
    for x in 2..6 {
        line.set(x, 6, 255);
    }
    let l = &contours(&line)[0];
    assert_eq!(l.len(), 6);
    assert_eq!(l[3], v(5.5, 6.5));
    assert_eq!(l[5], v(3.5, 6.5));
    assert!(contours(&Gray::new(5, 5)).is_empty());
    let bad = Gray {
        width: 2,
        height: 2,
        data: vec![1],
    };
    assert!(contours(&bad).is_empty());
}

#[test]
fn label_regions_splits_by_key_and_connectivity() {
    // Two diagonal pixels: one region with 8-connectivity, two with 4.
    let key = |i: usize| u32::from(i == 0 || i == 4);
    assert_eq!(label_regions(3, 3, true, key).1, 1);
    let (labels, count) = label_regions(3, 3, false, key);
    assert_eq!(count, 2);
    assert_eq!((labels[0], labels[4], labels[1]), (1, 2, 0));
    // Different keys never join.
    let (_, count) = label_regions(4, 1, false, |i| [1, 1, 2, 2][i]);
    assert_eq!(count, 2);
}

#[test]
fn simplify_keeps_only_the_shape() {
    let zigzag: Vec<Vec2> = (0..20)
        .map(|i| v(f64::from(i), if i % 2 == 0 { 0.0 } else { 0.4 }))
        .collect();
    assert_eq!(simplify(&zigzag, 0.5, false), vec![zigzag[0], zigzag[19]]);
    let bend = [v(0.0, 0.0), v(5.0, 0.1), v(10.0, 0.0), v(10.0, 10.0)];
    assert_eq!(
        simplify(&bend, 0.5, false),
        vec![bend[0], bend[2], bend[3]]
    );
    // A ring with a point every pixel collapses to its corners, in order, wherever it starts.
    let mut ring = Vec::new();
    for i in 0..10 {
        ring.push(v(f64::from(i), 0.0));
    }
    for i in 0..10 {
        ring.push(v(10.0, f64::from(i)));
    }
    for i in 0..10 {
        ring.push(v(10.0 - f64::from(i), 10.0));
    }
    for i in 0..10 {
        ring.push(v(0.0, 10.0 - f64::from(i)));
    }
    ring.rotate_left(5);
    let s = simplify(&ring, 0.5, true);
    assert_eq!(
        s,
        vec![v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0), v(0.0, 0.0)]
    );
    assert!(signed_area2(&s) > 0.0);
    assert_eq!(simplify(&ring[..2], 0.5, true), ring[..2].to_vec());
    assert_eq!(simplify(&[v(1.0, 1.0); 4], 0.5, true), vec![v(1.0, 1.0)]);
    assert_eq!(simplify(&bend, f64::NAN, false).len(), 4);
}

#[test]
fn convex_hull_drops_inner_collinear_and_duplicate_points() {
    let pts = [
        v(2.0, 2.0),
        v(0.0, 0.0),
        v(4.0, 4.0),
        v(4.0, 0.0),
        v(1.0, 3.0),
        v(2.0, 0.0),
        v(0.0, 4.0),
        v(4.0, 4.0),
        v(f64::NAN, 1.0),
    ];
    let hull = convex_hull(&pts);
    assert_eq!(
        hull,
        vec![v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(0.0, 4.0)]
    );
    assert!(signed_area2(&hull) > 0.0);
    assert_eq!(convex_hull(&[v(1.0, 1.0), v(0.0, 0.0)]).len(), 2);
    let line = [v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)];
    assert_eq!(convex_hull(&line), vec![v(0.0, 0.0), v(2.0, 2.0)]);
}

#[test]
fn polygon_area_ignores_orientation() {
    let sq = [v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(0.0, 4.0)];
    assert_eq!(polygon_area(&sq), 16.0);
    let mut rev = sq;
    rev.reverse();
    assert_eq!(polygon_area(&rev), 16.0);
    assert_eq!(polygon_area(&[v(0.0, 0.0), v(3.0, 0.0), v(0.0, 2.0)]), 3.0);
    assert_eq!(polygon_area(&sq[..2]), 0.0);
    assert_eq!(perimeter(&sq), 16.0);
    assert_eq!(perimeter(&sq[..1]), 0.0);
    assert_eq!(centroid(&sq), v(2.0, 2.0));
    assert_eq!(centroid(&[]), Vec2::ZERO);
    assert!(point_in_polygon(v(1.0, 1.0), &sq));
    assert!(!point_in_polygon(v(5.0, 1.0), &sq));
    assert_eq!(segment_distance(v(2.0, 3.0), v(0.0, 0.0), v(4.0, 0.0)), 3.0);
    assert_eq!(segment_distance(v(7.0, 4.0), v(0.0, 0.0), v(4.0, 0.0)), 5.0);
    assert_eq!(segment_distance(v(3.0, 4.0), v(0.0, 0.0), v(0.0, 0.0)), 5.0);
}

fn disc_edges(c: Vec2, r: f64) -> Gray {
    let photo = paint(120, 90, |_| [200.0; 3], &[(Shape::Disc(c, r), [60.0; 3])]);
    Prepared::new(&photo).edges
}

#[test]
fn hough_finds_a_drawn_circle_and_nothing_in_a_square() {
    let (c, r) = (v(57.3, 44.6), 21.0);
    let found = hough_circles(&disc_edges(c, r), 5.0, 40.0, 0.5);
    assert!(!found.is_empty());
    let (fc, fr, score) = found[0];
    assert!(fc.distance(c) < 0.5, "{fc:?}");
    assert!((fr - r).abs() < 0.5, "{fr}");
    assert!(score > 0.85, "{score}");
    let square = paint(
        120,
        90,
        |_| [200.0; 3],
        &[(
            Shape::Quad([v(30.0, 20.0), v(80.0, 20.0), v(80.0, 70.0), v(30.0, 70.0)]),
            [60.0; 3],
        )],
    );
    assert!(hough_circles(&Prepared::new(&square).edges, 5.0, 40.0, 0.5).is_empty());
    let edges = disc_edges(c, r);
    assert!(hough_circles(&edges, 30.0, 10.0, 0.5).is_empty());
    assert!(hough_circles(&edges, f64::NAN, 10.0, 0.5).is_empty());
    assert!(hough_circles(&Gray::new(50, 50), 5.0, 10.0, 0.5).is_empty());
    assert!(hough_circles(&Gray::new(3, 3), 5.0, 10.0, 0.5).is_empty());
    // A radius range that misses the circle finds nothing good enough.
    assert!(hough_circles(&edges, 5.0, 12.0, 0.5).is_empty());
    // With no threshold the circle is still the strongest.
    let any = hough_circles(&edges, 5.0, 40.0, f64::NAN);
    assert!(any[0].0.distance(c) < 0.5);
}

#[test]
fn order_quad_puts_corners_in_corner_pin_order() {
    let tl = v(10.0, 12.0);
    let tr = v(50.0, 8.0);
    let br = v(55.0, 40.0);
    let bl = v(8.0, 44.0);
    for scrambled in [[br, tl, bl, tr], [bl, br, tr, tl], [tl, tr, br, bl]] {
        assert_eq!(order_quad(scrambled), [tl, tr, br, bl]);
    }
    // A diamond still gets a stable, clockwise order.
    let d = order_quad([v(5.0, 0.0), v(10.0, 5.0), v(5.0, 10.0), v(0.0, 5.0)]);
    assert!(signed_area2(&d) > 0.0);
}

#[test]
fn fit_circle_recovers_exact_circles_and_rejects_lines() {
    let pts: Vec<Vec2> = (0..12)
        .map(|k| {
            let a = f64::from(k) * 0.5;
            v(3.0 + 7.0 * a.cos(), -2.0 + 7.0 * a.sin())
        })
        .collect();
    let (c, r) = fit_circle(&pts).unwrap();
    assert!(c.distance(v(3.0, -2.0)) < 1e-9 && (r - 7.0).abs() < 1e-9);
    assert!(fit_circle(&pts[..2]).is_none());
    assert!(fit_circle(&[v(0.0, 0.0), v(1.0, 1.0), v(2.0, 2.0)]).is_none());
    assert!(fit_circle(&[v(f64::NAN, 0.0), v(1.0, 1.0), v(2.0, 0.0)]).is_none());
}

#[test]
fn quads_fit_and_refine_onto_the_box_faces() {
    let s = scene();
    let prep = Prepared::new(&s.photo);
    for face in [s.top, s.left, s.right] {
        let c = centroid(&face);
        let inset: Vec<Vec2> = face.iter().map(|&p| p + (c - p) * 0.08).collect();
        let refined = refine_polygon(&prep, &inset, 4.0);
        let err = corner_error(&refined, &face);
        assert!(err < 0.75, "{face:?} refined to {refined:?} ({err})");
        assert!(edge_support(&prep, &face) > 0.9);
    }
    let blank = [v(140.0, 20.0), v(190.0, 20.0), v(190.0, 35.0), v(140.0, 35.0)];
    assert!(edge_support(&prep, &blank) < 0.05);
    assert_eq!(refine_polygon(&prep, &blank, 4.0), blank.to_vec());
    let fit = quad_from_outline(&s.right).unwrap();
    assert!(is_quad_like(&fit));
    assert_eq!(fit.quad, s.right);
    let round: Vec<Vec2> = (0..40)
        .map(|k| {
            let a = f64::from(k) * std::f64::consts::TAU / 40.0;
            v(50.0 + 20.0 * a.cos(), 50.0 + 20.0 * a.sin())
        })
        .collect();
    assert!(!is_quad_like(&quad_from_outline(&round).unwrap()));
    assert!(quad_from_outline(&round[..3]).is_none());
}

#[test]
fn circles_refine_onto_the_plate() {
    let s = scene();
    let prep = Prepared::new(&s.photo);
    let (c, r, coverage, rms) =
        refine_circle(&prep, s.disc.0 + v(1.5, -1.0), s.disc.1 - 2.0, 4.0).unwrap();
    assert!(c.distance(s.disc.0) < 0.3, "{c:?}");
    assert!((r - s.disc.1).abs() < 0.3, "{r}");
    assert!(coverage > 0.9 && rms < 0.3, "{coverage} {rms}");
    assert!(refine_circle(&prep, v(150.0, 25.0), 8.0, 3.0).is_none());
}

#[test]
fn zz_debug_square() {
    let square = paint(
        120,
        90,
        |_| [200.0; 3],
        &[(
            Shape::Quad([v(30.0, 20.0), v(80.0, 20.0), v(80.0, 70.0), v(30.0, 70.0)]),
            [60.0; 3],
        )],
    );
    let found = hough_circles(&Prepared::new(&square).edges, 5.0, 40.0, 0.0);
    for f in found.iter().take(8) {
        eprintln!("{f:?}");
    }
}

#[test]
fn prepared_handles_inconsistent_photos() {
    let bad = Rgba {
        width: 4,
        height: 4,
        data: vec![0; 5],
    };
    let p = Prepared::new(&bad);
    assert_eq!((p.w, p.h), (0, 0));
    assert_eq!(sample(&p.mag, 0, 0, v(1.0, 1.0)), 0.0);
    let g = Prepared::grey(&scene().photo);
    assert!(g.rgb[0].is_empty() && g.w == 320);
    let buf = [0.0f32, 10.0, 20.0, 30.0];
    assert_eq!(sample(&buf, 2, 2, v(1.0, 0.5)), 5.0);
    assert_eq!(sample(&buf, 2, 2, v(-5.0, 9.0)), 20.0);
    assert_eq!(sample(&buf, 2, 2, v(f64::NAN, 0.0)), 0.0);
}
