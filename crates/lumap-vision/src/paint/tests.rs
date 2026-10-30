#![allow(clippy::unwrap_used, clippy::float_cmp)]

use super::*;

const W: u32 = 160;
const H: u32 = 90;

fn alpha_at(out: &Rgba, x: u32, y: u32) -> u8 {
    out.get(x, y)[3]
}

/// A horizontal stroke from (20, 45) to (120, 45) drawn in small steps.
fn line(canvas: &mut Canvas, t0: f64) {
    for i in 0..=50 {
        canvas.pen(
            Some(Vec2::new(20.0 + f64::from(i) * 2.0, 45.0)),
            t0 + f64::from(i) * 0.01,
        );
    }
    canvas.pen(None, t0 + 0.6);
}

#[test]
fn every_brush_lights_the_path_and_nothing_far_from_it() {
    let mut canvas = Canvas::new(W, H);
    line(&mut canvas, 0.0);
    assert_eq!(canvas.strokes(), 1);
    let mut out = Rgba::default();
    for brush in Brush::ALL {
        canvas.render(&mut out, brush, [0.2, 0.6, 1.0, 1.0], 6.0, 0.0, 1.0);
        assert!(out.is_consistent() && out.width == W && out.height == H);
        if brush != Brush::Sparkle {
            for x in (20..=120).step_by(10) {
                assert!(alpha_at(&out, x, 45) > 200, "{brush:?} misses x={x}");
            }
        }
        let lit = (0..W).filter(|&x| alpha_at(&out, x, 45) > 0).count();
        assert!(lit > 40, "{brush:?} lit only {lit}");
        for x in 0..W {
            assert_eq!(
                alpha_at(&out, x, 5),
                0,
                "{brush:?} leaked to the top at {x}"
            );
            assert_eq!(
                alpha_at(&out, x, 85),
                0,
                "{brush:?} leaked to the bottom at {x}"
            );
        }
        for y in 0..H {
            assert_eq!(
                alpha_at(&out, 150, y),
                0,
                "{brush:?} leaked past the end at {y}"
            );
        }
    }
}

#[test]
fn caps_are_round_and_the_line_is_anti_aliased() {
    let mut canvas = Canvas::new(W, H);
    line(&mut canvas, 0.0);
    let mut out = Rgba::default();
    canvas.render(&mut out, Brush::Ink, [1.0; 4], 10.0, 0.0, 1.0);
    // The cap reaches past the last point by half the width...
    assert!(alpha_at(&out, 123, 45) > 200);
    // ...but rounds off at the corner of the bounding square.
    assert_eq!(alpha_at(&out, 124, 49), 0);
    // The edge has partial coverage.
    let edge: Vec<u8> = (38..=52).map(|y| alpha_at(&out, 70, y)).collect();
    assert!(edge.iter().any(|&a| a > 0 && a < 255), "{edge:?}");
    assert_eq!(out.get(70, 45), [255, 255, 255, 255]);
}

#[test]
fn points_are_thinned_and_straight_runs_merge() {
    let mut canvas = Canvas::new(W, H);
    for i in 0..100 {
        canvas.pen(Some(Vec2::new(10.0 + f64::from(i) * 0.5, 20.0)), 0.0);
    }
    let points = canvas.strokes[0].points.len();
    // 50 px of straight line at 0.5 px steps: thinned and merged to a handful of points.
    assert!(points <= 4, "{points}");
    assert!((canvas.strokes[0].length() - 49.5).abs() < 1.6);
    // A curve keeps its shape.
    let mut curve = Canvas::new(W, H);
    for i in 0..=90 {
        let a = f64::from(i) / 90.0 * std::f64::consts::PI;
        curve.pen(
            Some(Vec2::new(80.0 + 30.0 * a.cos(), 45.0 + 30.0 * a.sin())),
            0.0,
        );
    }
    assert!(curve.strokes[0].points.len() > 10);
}

#[test]
fn pen_up_jumps_and_bad_points_start_new_strokes() {
    let mut canvas = Canvas::new(W, H);
    canvas.pen(Some(Vec2::new(10.0, 10.0)), 0.0);
    canvas.pen(Some(Vec2::new(20.0, 10.0)), 0.1);
    canvas.pen(None, 0.2);
    canvas.pen(Some(Vec2::new(30.0, 30.0)), 0.3);
    assert_eq!(canvas.strokes(), 2);
    // A jump across the frame is a new stroke.
    canvas.pen(Some(Vec2::new(150.0, 80.0)), 0.4);
    assert_eq!(canvas.strokes(), 3);
    // Non-finite positions lift the pen.
    canvas.pen(Some(Vec2::new(f64::NAN, 1.0)), 0.5);
    canvas.pen(Some(Vec2::new(152.0, 80.0)), 0.6);
    assert_eq!(canvas.strokes(), 4);
    // A tap draws a dot.
    let mut out = Rgba::default();
    canvas.render(&mut out, Brush::Neon, [1.0; 4], 6.0, 0.0, 1.0);
    assert!(alpha_at(&out, 30, 30) > 200);
    canvas.clear();
    assert_eq!(canvas.strokes(), 0);
    canvas.render(&mut out, Brush::Neon, [1.0; 4], 6.0, 0.0, 1.0);
    assert!(out.data.iter().all(|&v| v == 0));
}

#[test]
fn fading_dims_old_strokes_and_drops_them_on_the_next_pen_call() {
    let mut canvas = Canvas::new(W, H);
    line(&mut canvas, 0.0);
    let mut out = Rgba::default();
    // Half way through a 2 s fade the stroke is about half as strong.
    canvas.render(&mut out, Brush::Ink, [1.0; 4], 6.0, 2.0, 1.5);
    let half = alpha_at(&out, 70, 45);
    assert!((100..160).contains(&half), "{half}");
    // Fully faded: nothing drawn, and the next pen call drops it.
    canvas.render(&mut out, Brush::Ink, [1.0; 4], 6.0, 2.0, 3.0);
    assert!(out.data.iter().all(|&v| v == 0));
    assert_eq!(canvas.strokes(), 1);
    canvas.pen(None, 3.0);
    assert_eq!(canvas.strokes(), 0);
    // A zero fade keeps strokes forever.
    line(&mut canvas, 10.0);
    canvas.render(&mut out, Brush::Ink, [1.0; 4], 6.0, 0.0, 1000.0);
    canvas.pen(None, 1000.0);
    assert_eq!(canvas.strokes(), 1);
    assert!(alpha_at(&out, 70, 45) > 200);
}

#[test]
fn sparkle_twinkles_with_time_and_rainbow_changes_hue_along_the_stroke() {
    let mut canvas = Canvas::new(W, H);
    line(&mut canvas, 0.0);
    let (mut a, mut b, mut c) = (Rgba::default(), Rgba::default(), Rgba::default());
    canvas.render(&mut a, Brush::Sparkle, [1.0; 4], 6.0, 0.0, 1.0);
    canvas.render(&mut b, Brush::Sparkle, [1.0; 4], 6.0, 0.0, 1.0);
    canvas.render(&mut c, Brush::Sparkle, [1.0; 4], 6.0, 0.0, 1.7);
    assert_eq!(a, b);
    assert_ne!(a, c);
    let mut r = Rgba::default();
    canvas.render(&mut r, Brush::Rainbow, [1.0, 1.0, 1.0, 1.0], 6.0, 0.0, 0.0);
    let start = r.get(22, 45);
    let mid = r.get(60, 45);
    assert_ne!(&start[..3], &mid[..3]);
    let mut later = Rgba::default();
    canvas.render(
        &mut later,
        Brush::Rainbow,
        [1.0, 1.0, 1.0, 1.0],
        6.0,
        0.0,
        0.5,
    );
    assert_ne!(r, later);
}

#[test]
fn long_strokes_carry_on_and_clones_are_independent() {
    let mut canvas = Canvas::new(4000, 100);
    let mut x = 0.0;
    for i in 0..(MAX_POINTS + 10) {
        // A zig-zag so no points merge.
        let y = if i % 2 == 0 { 40.0 } else { 44.0 };
        canvas.pen(Some(Vec2::new(x, y)), 0.0);
        x += 0.9;
    }
    assert_eq!(canvas.strokes(), 2);
    let copy = canvas.clone();
    canvas.clear();
    assert_eq!(copy.strokes(), 2);
}
