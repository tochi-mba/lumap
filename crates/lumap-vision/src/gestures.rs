//! Hand gestures from the 21 landmarks of a tracked hand: the pinch that puts the pen down, an
//! open hand, a fist, a point, a thumbs-up and quick sideways swipes. Pure and model-free: the
//! landmarks come from the hand tracker (the `hands` module, feature `hands`) or from anything
//! else that uses MediaPipe's landmark order, which is how the tests drive it.
//!
//! Every reading is a confidence in `0..=1` for lumap's camera map, where `0.5` and up counts as
//! pressed. Poses and the pinch switch with hysteresis, so a hand resting near a threshold does
//! not chatter; swipes are one-reading pulses. While the pen is down (pinching) every other
//! gesture reads zero, so a fast stroke never swipes the show along and a hand closing
//! mid-stroke never clears the paint. Distances are measured in palm sizes, so none of this
//! depends on how far the hand is from the camera.

use lumap_core::cammap::Gesture;
use lumap_core::Vec2;

const WRIST: usize = 0;
const THUMB_MCP: usize = 2;
const THUMB_IP: usize = 3;
const THUMB_TIP: usize = 4;
const INDEX_MCP: usize = 5;
const INDEX_TIP: usize = 8;
const PINKY_MCP: usize = 17;
/// Knuckle (MCP), PIP, DIP and tip of the index, middle, ring and little fingers.
const FINGERS: [[usize; 4]; 4] = [[5, 6, 7, 8], [9, 10, 11, 12], [13, 14, 15, 16], [17, 18, 19, 20]];

/// Thumb and index tips closer than this many palm sizes put the pen down...
const PINCH_CLOSE: f64 = 0.25;
/// ...and only farther apart than this do they lift it again.
const PINCH_OPEN: f64 = 0.40;
/// A pose switches on when its score reaches this...
const POSE_ON: f64 = 0.65;
/// ...and off when it falls to this.
const POSE_OFF: f64 = 0.35;
/// Seconds of wrist positions a swipe is judged over.
const SWIPE_WINDOW: f64 = 0.3;
/// A swipe travels at least this many palm sizes sideways...
const SWIPE_DISTANCE: f64 = 1.0;
/// ...at this many palm sizes per second or faster...
const SWIPE_SPEED: f64 = 4.0;
/// ...mostly sideways (horizontal travel at least this many times the vertical)...
const SWIPE_STRAIGHTNESS: f64 = 2.0;
/// ...and spread over several frames: no single step may cover more than this share of it, so
/// a tracker jumping between two hands is not a swipe.
const SWIPE_MAX_STEP: f64 = 0.6;
/// Seconds after a swipe before another can fire, so the hand coming back is not one.
const SWIPE_COOLDOWN: f64 = 0.5;
/// Wrist positions kept: more than [`SWIPE_WINDOW`] holds at 60 frames a second.
const TRAIL: usize = 32;

/// One tracked hand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandPose {
    /// The 21 landmarks in camera pixels, in MediaPipe's order: 0 wrist; 1 to 4 the thumb (CMC,
    /// MCP, IP, tip); 5 to 8 the index finger (knuckle, PIP, DIP, tip); 9 to 12 the middle
    /// finger, 13 to 16 the ring finger and 17 to 20 the little finger, in the same order.
    pub landmarks: [Vec2; 21],
    /// Which hand, as the landmark model reports it: `0` left to `1` right. The model assumes
    /// a mirrored, selfie-style image, so for a camera that is not mirrored the two swap.
    pub handedness: f64,
    /// How sure the tracker is that this is a hand, `0..=1`.
    pub score: f64,
}

/// How strongly one gesture is present.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GestureReading {
    /// The gesture.
    pub gesture: Gesture,
    /// Confidence `0..=1`; `0.5` and up means the gesture is on.
    pub value: f64,
}

/// Where the pen is: half way between the thumb and index fingertips, in camera pixels.
pub fn pinch_point(hand: &HandPose) -> Vec2 {
    hand.landmarks[THUMB_TIP].lerp(hand.landmarks[INDEX_TIP], 0.5)
}

/// Turns a stream of hand poses into gesture readings, one call per camera frame.
#[derive(Debug, Clone, Default)]
pub struct GestureClassifier {
    pinch: Latch,
    open_palm: Latch,
    fist: Latch,
    thumbs_up: Latch,
    point: Latch,
    trail: Trail,
    /// Time before which no new swipe fires.
    quiet_until: f64,
}

impl GestureClassifier {
    /// A classifier that has seen no hand yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Readings for the hand seen at time `t` (seconds, increasing), one per gesture in
    /// [`Gesture::ALL`] order. Everything reads zero without a hand (or with a degenerate one),
    /// and the next hand starts afresh.
    pub fn update(&mut self, hand: Option<&HandPose>, t: f64) -> Vec<GestureReading> {
        let readings = match hand.and_then(Shape::measure) {
            Some(shape) => self.classify(&shape, t),
            None => {
                self.forget();
                [0.0; 7]
            }
        };
        Gesture::ALL
            .iter()
            .zip(readings)
            .map(|(&gesture, value)| GestureReading { gesture, value })
            .collect()
    }

    /// Values in [`Gesture::ALL`] order.
    fn classify(&mut self, shape: &Shape, t: f64) -> [f64; 7] {
        let pinch = self.pinch.update(shape.pinch, 1.0, 0.0);
        let pen_down = self.pinch.on;
        let curled = shape.fingers.iter().fold(1.0_f64, |m, e| m.min(1.0 - e));
        let pose = |latch: &mut Latch, raw: f64| {
            latch.update(if pen_down { 0.0 } else { raw }, POSE_ON, POSE_OFF)
        };
        let open_palm = pose(
            &mut self.open_palm,
            shape.fingers.iter().fold(1.0_f64, |m, &e| m.min(e)),
        );
        let fist = pose(&mut self.fist, curled.min(1.0 - shape.thumb));
        let thumbs_up = pose(
            &mut self.thumbs_up,
            curled.min(shape.thumb).min(shape.thumb_up),
        );
        let [index, middle, ring, little] = shape.fingers;
        let point = pose(
            &mut self.point,
            index.min(1.0 - middle).min(1.0 - ring).min(1.0 - little),
        );
        let (left, right) = if pen_down {
            self.trail.clear();
            (0.0, 0.0)
        } else {
            self.swipe(shape, t)
        };
        [pinch, open_palm, fist, left, right, thumbs_up, point]
    }

    /// `(left, right)` pulses from the wrist's recent travel.
    fn swipe(&mut self, shape: &Shape, t: f64) -> (f64, f64) {
        self.trail.push(t, shape.wrist);
        if t < self.quiet_until {
            return (0.0, 0.0);
        }
        let Some(travel) = self.trail.travel(t - SWIPE_WINDOW) else {
            return (0.0, 0.0);
        };
        let palm = shape.palm;
        let sideways = travel.delta.x.abs();
        let swiped = sideways >= SWIPE_DISTANCE * palm
            && sideways >= SWIPE_SPEED * palm * travel.span
            && sideways >= SWIPE_STRAIGHTNESS * travel.delta.y.abs()
            && travel.longest_step <= SWIPE_MAX_STEP * sideways;
        if !swiped {
            return (0.0, 0.0);
        }
        self.trail.clear();
        self.quiet_until = t + SWIPE_COOLDOWN;
        if travel.delta.x < 0.0 {
            (1.0, 0.0)
        } else {
            (0.0, 1.0)
        }
    }

    /// The hand is gone: every gesture switches off and the wrist trail starts again.
    fn forget(&mut self) {
        for latch in [
            &mut self.pinch,
            &mut self.open_palm,
            &mut self.fist,
            &mut self.thumbs_up,
            &mut self.point,
        ] {
            latch.on = false;
        }
        self.trail.clear();
    }
}

/// An on/off state with hysteresis over a raw score in `0..=1`.
#[derive(Debug, Clone, Copy, Default)]
struct Latch {
    on: bool,
}

impl Latch {
    /// Switches on at `raw >= on_at` and off at `raw <= off_at`, and reports the score so that it
    /// is at least `0.5` exactly while on: `off_at..=1` maps onto `0.5..=1` while on and
    /// `0..on_at` onto `0..0.5` while off.
    fn update(&mut self, raw: f64, on_at: f64, off_at: f64) -> f64 {
        if self.on {
            self.on = raw > off_at;
        } else {
            self.on = raw >= on_at;
        }
        if self.on {
            0.5 + 0.5 * ((raw - off_at) / (1.0 - off_at)).clamp(0.0, 1.0)
        } else {
            0.5 * (raw / on_at).clamp(0.0, 1.0)
        }
    }
}

/// What the classifier needs to know about one hand, scale-free.
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// Mean wrist-to-knuckle distance in pixels: the unit everything else is measured in.
    palm: f64,
    wrist: Vec2,
    /// How far out each of the index, middle, ring and little fingers reaches: `0` curled into
    /// the palm, `1` straight out.
    fingers: [f64; 4],
    /// `0` thumb tucked across the palm, `1` thumb out.
    thumb: f64,
    /// `1` when the thumb points up the image, `0` when it points sideways or down.
    thumb_up: f64,
    /// `1` when the thumb and index tips touch (with the index reaching out of the palm, which a
    /// fist's does not), falling to `0` once they are [`PINCH_OPEN`] apart.
    pinch: f64,
}

impl Shape {
    fn measure(hand: &HandPose) -> Option<Self> {
        let lm = &hand.landmarks;
        if !lm.iter().all(|p| p.is_finite()) {
            return None;
        }
        let wrist = lm[WRIST];
        let palm = FINGERS
            .iter()
            .map(|f| lm[f[0]].distance(wrist))
            .sum::<f64>()
            / 4.0;
        if palm < 1e-6 {
            return None;
        }
        let fingers = FINGERS.map(|[knuckle, _, _, tip]| {
            let along = lm[knuckle] - wrist;
            let reach = (lm[tip] - lm[knuckle]).dot(along) / along.length().max(1e-9) / palm;
            smoothstep(0.2, 0.5, reach)
        });
        // An extended thumb points away from the little finger; a tucked one crosses towards it.
        let thumb_out = (lm[THUMB_TIP].distance(lm[PINKY_MCP])
            - lm[THUMB_IP].distance(lm[PINKY_MCP]))
            / palm;
        let thumb = lm[THUMB_TIP] - lm[THUMB_MCP];
        let thumb_up = -thumb.y / thumb.length().max(1e-9);
        let gap = lm[THUMB_TIP].distance(lm[INDEX_TIP]) / palm;
        let closeness = ((PINCH_OPEN - gap) / (PINCH_OPEN - PINCH_CLOSE)).clamp(0.0, 1.0);
        let index_out = lm[INDEX_TIP].distance(wrist) / lm[INDEX_MCP].distance(wrist).max(1e-9);
        Some(Self {
            palm,
            wrist,
            fingers,
            thumb: smoothstep(-0.05, 0.1, thumb_out),
            thumb_up: smoothstep(0.35, 0.75, thumb_up),
            pinch: closeness * smoothstep(0.9, 1.1, index_out),
        })
    }
}

/// `0` below `edge0`, `1` above `edge1`, a smooth S between.
fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Recent wrist positions, oldest first, in a fixed ring (no allocation per frame).
#[derive(Debug, Clone, Copy)]
struct Trail {
    samples: [(f64, Vec2); TRAIL],
    start: usize,
    len: usize,
}

impl Default for Trail {
    fn default() -> Self {
        Self {
            samples: [(0.0, Vec2::ZERO); TRAIL],
            start: 0,
            len: 0,
        }
    }
}

/// Net movement over the recent trail.
#[derive(Debug, Clone, Copy)]
struct Travel {
    delta: Vec2,
    /// Seconds between the first and last sample.
    span: f64,
    /// Length of the longest single step.
    longest_step: f64,
}

impl Trail {
    fn get(&self, i: usize) -> (f64, Vec2) {
        self.samples[(self.start + i) % TRAIL]
    }

    fn clear(&mut self) {
        self.len = 0;
    }

    /// Adds a sample; a clock that runs backwards starts the trail again and a repeated time
    /// is ignored.
    fn push(&mut self, t: f64, p: Vec2) {
        if self.len > 0 {
            let (last, _) = self.get(self.len - 1);
            if t < last {
                self.clear();
            } else if t <= last {
                return;
            }
        }
        if self.len == TRAIL {
            self.start = (self.start + 1) % TRAIL;
            self.len -= 1;
        }
        self.samples[(self.start + self.len) % TRAIL] = (t, p);
        self.len += 1;
    }

    /// Drops samples older than `since`, then measures the rest; `None` with fewer than three.
    fn travel(&mut self, since: f64) -> Option<Travel> {
        while self.len > 0 && self.get(0).0 < since {
            self.start = (self.start + 1) % TRAIL;
            self.len -= 1;
        }
        if self.len < 3 {
            return None;
        }
        let (t0, first) = self.get(0);
        let (t1, last) = self.get(self.len - 1);
        let longest_step = (1..self.len)
            .map(|i| self.get(i).1.distance(self.get(i - 1).1))
            .fold(0.0, f64::max);
        Some(Travel {
            delta: last - first,
            span: t1 - t0,
            longest_step,
        })
    }
}

