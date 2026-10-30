use super::*;

/// A camera at half the projector's resolution looking straight at it: camera pixel (x, y)
/// sees projector pixel (2x + 1, 2y + 1).
fn straight(cam_w: u32, cam_h: u32) -> Correspondence {
    let mut proj = Vec::with_capacity((cam_w * cam_h) as usize);
    for y in 0..cam_h {
        for x in 0..cam_w {
            proj.push([(2 * x + 1) as f32, (2 * y + 1) as f32]);
        }
    }
    Correspondence {
        cam_w,
        cam_h,
        proj_w: cam_w * 2,
        proj_h: cam_h * 2,
        confidence: vec![255; proj.len()],
        proj,
    }
}

fn error_at(map: &ProjectorMap, x: u32, y: u32) -> Option<f64> {
    let c = map.get(x, y)?;
    let want = Vec2::new(f64::from(x) / 2.0, f64::from(y) / 2.0);
    Some(((c.x - want.x).powi(2) + (c.y - want.y).powi(2)).sqrt())
}

#[test]
fn inversion_fills_every_projector_pixel_the_camera_spans() {
    let map = straight(160, 90).invert();
    let mut worst: f64 = 0.0;
    for y in 0..180 {
        for x in 0..320 {
            let e = error_at(&map, x, y).expect("filled");
            worst = worst.max(e);
        }
    }
    assert!(worst < 1.5, "worst {worst}");
}

#[test]
fn an_unreadable_strip_is_bridged_and_stray_light_ignored() {
    let mut c = straight(160, 90);
    // Rows 40..46 unreadable (a stripe boundary the camera blurred).
    for y in 40..46 {
        for x in 0..160 {
            c.proj[y * 160 + x] = [f32::NAN; 2];
        }
    }
    // Light bounced onto the far right decodes as the projector's top-left corner region.
    for y in 60..90 {
        for x in 140..160 {
            c.proj[y * 160 + x] = [(x - 140) as f32 * 3.0, (y - 60) as f32 * 2.0];
        }
    }
    let map = c.invert();
    // The top-left keeps its own camera position, not one pulled toward the stray patch.
    for (x, y) in [(10, 10), (40, 20), (55, 70)] {
        let e = error_at(&map, x, y).expect("filled");
        assert!(e < 2.0, "({x}, {y}) off by {e}");
    }
    // The strip is filled from either side, close to where it really is.
    for x in [20, 100, 200, 300] {
        let e = error_at(&map, x, 86).expect("bridged");
        assert!(e < 3.0, "strip at {x} off by {e}");
    }
}

#[test]
fn a_lone_misread_is_dropped() {
    let mut c = straight(40, 40);
    c.proj[20 * 40 + 20] = [70.0, 3.0];
    assert!(!c.agrees(20, 20, Vec2::new(70.0, 3.0)));
    assert!(c.agrees(10, 10, Vec2::new(21.0, 21.0)));
    let map = c.invert();
    let e = error_at(&map, 70, 3).expect("filled");
    assert!(e < 2.0, "misread pulled (70, 3) off by {e}");
}
