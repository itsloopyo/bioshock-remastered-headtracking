//! Per-axis interpolation + smoothing for head-tracking samples.
//!
//! Bridges a low-rate tracker (typically 60Hz off a phone) to a
//! high-refresh display (120/144/240Hz). Without this, every other
//! frame on a 120Hz display reads the same atomic value the receiver
//! wrote, so the camera advances on a 60Hz beat while the rest of the
//! scene moves at 120Hz - the eye reads that uneven cadence as low
//! framerate.
//!
//! Mirrors the canonical `cameraunlock-core/cpp` `PoseInterpolator` +
//! `SmoothingUtils` (the BSR mod doesn't link the C++ core, so we
//! port).
//!
//! Pipeline per render frame:
//!   raw atomics (yaw,pitch,roll, x,y,z) + sample-sequence counter
//!     -> per-axis Interpolator (lerp between successive samples,
//!        EMA-estimated sample interval, velocity extrapolation up to
//!        half a sample period past the latest known value)
//!     -> per-axis Smoother (frame-rate independent exponential, using
//!        the connection-selected LocalSmoothing / RemoteSmoothing value)
//!     -> consumed by engine_hook (FRotator / FVector) and the D3D
//!        overlay (reticle projection)
//!
//! State lives behind a `parking_lot::Mutex`. Engine_hook holds it for
//! the duration of one `tick_frame` call; the hotkey thread holds it
//! briefly on recenter to clear state. Contention is effectively zero
//! since both consumers run on different threads with very different
//! cadences.

use std::sync::atomic::Ordering;
use std::time::Instant;

use parking_lot::Mutex;

use crate::tracking::{
    get_recentered_position_atomic, get_recentered_rotation_atomic, ATOMIC_SAMPLE_SEQ,
    ATOMIC_SMOOTHED_POSITION, ATOMIC_SMOOTHED_ROTATION,
};

const INTERVAL_BLEND: f64 = 0.3;

/// Assumed interval between tracker samples until real ones are observed.
/// Matches `kDefaultSampleInterval` in the C++ core and
/// `DefaultSampleInterval` in C#.
///
/// The EMA only learns the true rate from a gap wider than
/// `MIN_SAMPLE_INTERVAL`, so a session that opens with two packets in quick
/// succession - a connect burst, or two datagrams landing between two
/// render frames - runs its whole first segment on this default. At 1/60 a
/// 30Hz tracker's first segment advanced at twice the true rate: it reached
/// the extrapolation cap halfway through the sample period and sat 50%
/// past the pose the tracker had reported for the rest of it. That is the
/// first-second-of-session jolt native mods had and Unity mods did not, and
/// it recurs after every recenter, because a recenter resets the estimate.
/// Guessing slow is safe (the segment lands short and the next sample
/// corrects it); guessing fast overshoots.
const DEFAULT_SAMPLE_INTERVAL: f64 = 1.0 / 30.0;

const MIN_SAMPLE_INTERVAL: f64 = 0.001;
const MAX_SAMPLE_INTERVAL: f64 = 0.2;
const MAX_EXTRAPOLATION_FRACTION: f64 = 0.5;

/// Frame delta assumed on the very first tick, when there is no previous
/// frame to measure against. A frame length, not a sample interval: the two
/// were the same constant, so changing the sample-interval default silently
/// doubled the first frame's dt.
const FIRST_FRAME_DT: f64 = 1.0 / 60.0;

/// Seconds a sample may be late before the extrapolation starts expiring.
/// Sized to outlast an ordinary Wi-Fi loss burst (50-200ms), because a
/// dropped packet or two is still a live feed and must behave exactly as
/// it did before: continue the prediction, then hold. Retreating on a
/// dropped packet would pull the camera BACKWARDS against a head that is
/// still turning, which reads far worse than the flat spot it replaces.
const EXTRAPOLATION_HOLD: f64 = 0.25;

/// Seconds over which a genuinely stalled feed converges back to the last
/// reported sample. Long enough that the correction is a drift, not a snap.
const EXTRAPOLATION_DECAY: f64 = 0.35;

/// Segment position to sample at, given interpolation progress and how
/// long the next sample has been outstanding.
///
/// Progress past 1.0 is extrapolation: a short prediction that keeps
/// velocity continuous between samples. It is only a prediction, so it
/// must not outlive the sample it predicted from. Clamping and then
/// HOLDING parks the output at 1.5x the last reported pose forever
/// whenever samples stop arriving - a tracker app streaming its last
/// value while the face is lost, or a head so still that consecutive
/// samples are bit-identical and never bump the sample sequence. A 25
/// degree head turn then renders as 37.5 degrees and stays there.
///
/// So the prediction expires, but on a WALL CLOCK rather than on
/// progress: progress is measured in units of an estimated sample
/// interval, and that estimate is stale by construction in exactly the
/// stall case, because the EMA only updates when a new sample arrives.
/// Below the hold threshold this is bit-for-bit the old behaviour; past
/// it the segment position eases - smoothstep, so there is no velocity
/// step at either end - to 1.0, the pose the tracker actually reported.
/// Matches `PoseInterpolator::SegmentPosition` in the C++ core.
fn segment_position(progress: f64, time_since_last_sample: f64) -> f64 {
    if progress < 0.0 {
        return 0.0;
    }
    let pt = progress.min(1.0 + MAX_EXTRAPOLATION_FRACTION);
    if time_since_last_sample <= EXTRAPOLATION_HOLD {
        return pt;
    }

    let u = ((time_since_last_sample - EXTRAPOLATION_HOLD) / EXTRAPOLATION_DECAY).min(1.0);
    let eased = u * u * (3.0 - 2.0 * u);
    pt + (1.0 - pt) * eased
}

/// Default smoothing for a tracker running on this machine (loopback).
/// Matches `DefaultLocalSmoothing` in the C# core /
/// `kDefaultLocalSmoothing` in the C++ core.
pub const DEFAULT_LOCAL_SMOOTHING: f64 = 0.0;

/// Default smoothing for a remote device sending over the network.
/// Matches `DefaultRemoteSmoothing` / `kDefaultRemoteSmoothing`.
pub const DEFAULT_REMOTE_SMOOTHING: f64 = 0.15;

/// Select the smoothing value for the current connection. This is the
/// only path by which a smoothing value reaches the smoother; never pick
/// with an `if` at the call site.
#[inline]
pub fn get_effective_smoothing(
    local_smoothing: f64,
    remote_smoothing: f64,
    is_remote_connection: bool,
) -> f64 {
    if is_remote_connection {
        remote_smoothing
    } else {
        local_smoothing
    }
}

/// Asymmetric per-axis position limits, in centimetres. More forward than
/// back so the player can lean toward the screen without the camera
/// clipping through the player model behind them, and far less down than
/// up for the same reason. `forward` is positive here: OpenTrack's `+Z`
/// (away from the screen) is negated at the receiver boundary, in
/// `get_recentered_position_atomic`.
pub const POS_LIMIT_FORWARD_CM: f64 = 40.0;
pub const POS_LIMIT_BACK_CM: f64 = 10.0;
pub const POS_LIMIT_SIDE_CM: f64 = 30.0;
pub const POS_LIMIT_UP_CM: f64 = 20.0;
pub const POS_LIMIT_DOWN_CM: f64 = 5.0;

/// Lower clamp on per-frame dt. Prevents division-by-near-zero in the
/// progress integration if two ticks land in the same microsecond.
const MIN_FRAME_DT: f64 = 0.0001;

/// Upper clamp on per-frame dt. Caps catch-up after a stall (alt-tab,
/// pause, debug breakpoint) so the interpolator can't fling itself
/// past the latest sample.
const MAX_FRAME_DT: f64 = 0.1;

/// Normalises an angle in degrees to -180..180. Matches `NormalizeAngle` in
/// the core's `angle_utils.h`.
fn normalize_angle(angle: f64) -> f64 {
    if (-180.0..=180.0).contains(&angle) {
        return angle;
    }
    let wrapped = angle % 360.0;
    if wrapped > 180.0 {
        wrapped - 360.0
    } else if wrapped < -180.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

/// Shortest signed angular distance from `from` to `to`, in degrees.
/// Matches `ShortestAngleDelta` in the core's `angle_utils.h`.
fn shortest_angle_delta(from: f64, to: f64) -> f64 {
    normalize_angle(to - from)
}

/// Whether an axis wraps at the +/-180 degree seam.
///
/// Yaw and roll do. Pitch does not: a tracker derives it from asin, which
/// bounds it to +/-90. Position axes are centimetres and MUST NOT, or a
/// 200cm reading would come out as -160cm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisKind {
    Angular,
    Linear,
}

impl AxisKind {
    /// Move `t` of the way from `from` to `to`.
    fn interpolate(self, from: f64, to: f64, t: f64) -> f64 {
        match self {
            // Lerping the raw scalar difference sends a 1 degree movement
            // across the seam, 179.5 to -179.5, the long way round instead:
            // -359 degrees, through every heading in between.
            Self::Angular => normalize_angle(from + shortest_angle_delta(from, to) * t),
            Self::Linear => from + (to - from) * t,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Interpolator {
    kind: AxisKind,
    from: f64,
    to: f64,
    progress: f64,
    sample_interval: f64,
    time_since_last_sample: f64,
    has_first_sample: bool,
    has_second_sample: bool,
}

impl Interpolator {
    const fn new(kind: AxisKind) -> Self {
        Self {
            kind,
            from: 0.0,
            to: 0.0,
            progress: 0.0,
            sample_interval: DEFAULT_SAMPLE_INTERVAL,
            time_since_last_sample: 0.0,
            has_first_sample: false,
            has_second_sample: false,
        }
    }

    const fn angular() -> Self {
        Self::new(AxisKind::Angular)
    }

    const fn linear() -> Self {
        Self::new(AxisKind::Linear)
    }

    fn reset(&mut self) {
        *self = Self::new(self.kind);
    }

    fn update(&mut self, raw: f64, is_new_sample: bool, dt: f64) -> f64 {
        self.time_since_last_sample += dt;

        if is_new_sample {
            if !self.has_first_sample {
                self.from = raw;
                self.to = raw;
                self.progress = 1.0;
                self.time_since_last_sample = 0.0;
                self.has_first_sample = true;
                return raw;
            }

            if self.time_since_last_sample > MIN_SAMPLE_INTERVAL {
                if !self.has_second_sample {
                    self.sample_interval = self.time_since_last_sample;
                    self.has_second_sample = true;
                } else {
                    self.sample_interval +=
                        (self.time_since_last_sample - self.sample_interval) * INTERVAL_BLEND;
                }
                self.sample_interval = self
                    .sample_interval
                    .clamp(MIN_SAMPLE_INTERVAL, MAX_SAMPLE_INTERVAL);
            }

            // Capture the current (possibly extrapolated) position as
            // the new segment's start so velocity stays continuous
            // across sample boundaries.
            let t = segment_position(self.progress, self.time_since_last_sample);
            self.from = self.kind.interpolate(self.from, self.to, t);

            self.to = raw;
            self.progress = 0.0;
            self.time_since_last_sample = 0.0;
        }

        if !self.has_first_sample {
            return raw;
        }

        self.progress += dt / self.sample_interval;

        let pt = segment_position(self.progress, self.time_since_last_sample);
        self.kind.interpolate(self.from, self.to, pt)
    }
}

#[derive(Debug, Clone, Copy)]
struct Smoother {
    kind: AxisKind,
    current: f64,
    has_value: bool,
}

impl Smoother {
    const fn new(kind: AxisKind) -> Self {
        Self {
            kind,
            current: 0.0,
            has_value: false,
        }
    }

    const fn angular() -> Self {
        Self::new(AxisKind::Angular)
    }

    const fn linear() -> Self {
        Self::new(AxisKind::Linear)
    }

    fn reset(&mut self) {
        *self = Self::new(self.kind);
    }

    /// Angular axes take the shortest path around the seam here too, not
    /// just in the interpolator. Matches `SmoothAngle` in the core's
    /// `smoothing_utils.h`, which the core's TrackingProcessor uses for yaw
    /// and roll while pitch uses the plain scalar `Smooth`.
    fn update(&mut self, target: f64, smoothing: f64, dt: f64) -> f64 {
        if !self.has_value {
            self.current = target;
            self.has_value = true;
            return target;
        }
        // 0..1 maps to speeds 50..0.1. Matches SmoothingUtils.cs / .h.
        let speed = lerp(50.0, 0.1, smoothing).clamp(0.1, 50.0);
        let t = 1.0 - (-speed * dt).exp();
        self.current = self.kind.interpolate(self.current, target, t);
        self.current
    }
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// One position axis, from raw centimetres to a limit-bounded offset.
#[derive(Debug, Clone, Copy)]
struct PositionAxis {
    interp: Interpolator,
    smooth: Smoother,
    min: f64,
    max: f64,
}

impl PositionAxis {
    const fn new(min: f64, max: f64) -> Self {
        Self {
            interp: Interpolator::linear(),
            smooth: Smoother::linear(),
            min,
            max,
        }
    }

    fn reset(&mut self) {
        self.interp.reset();
        self.smooth.reset();
    }

    /// Clamped BEFORE smoothing as well as after. The smoothing state used
    /// to track the unclamped input, so a lean past the limits drove it far
    /// outside them and it then sat saturated on the way back, pinning the
    /// output at the limit for hundreds of milliseconds after the head had
    /// returned. Clamping an already-bounded input is a no-op, so ordinary
    /// movement is unchanged. Matches PositionProcessor in the core.
    fn update(&mut self, raw: f64, is_new_sample: bool, dt: f64, smoothing: f64) -> f64 {
        let interpolated = self
            .interp
            .update(raw, is_new_sample, dt)
            .clamp(self.min, self.max);
        self.smooth
            .update(interpolated, smoothing, dt)
            .clamp(self.min, self.max)
    }
}

struct Pipeline {
    rot: [Interpolator; 3],
    rot_smooth: [Smoother; 3],
    pos: [PositionAxis; 3],
    last_frame: Option<Instant>,
    last_seen_seq: u64,
}

impl Pipeline {
    const fn new() -> Self {
        Self {
            // Yaw and roll wrap at the seam; pitch is asin-bounded to +/-90
            // and cannot. Order matches the tuples from
            // get_recentered_rotation_atomic: yaw, pitch, roll.
            rot: [
                Interpolator::angular(),
                Interpolator::linear(),
                Interpolator::angular(),
            ],
            rot_smooth: [Smoother::angular(), Smoother::linear(), Smoother::angular()],
            pos: [
                PositionAxis::new(-POS_LIMIT_SIDE_CM, POS_LIMIT_SIDE_CM),
                PositionAxis::new(-POS_LIMIT_DOWN_CM, POS_LIMIT_UP_CM),
                PositionAxis::new(-POS_LIMIT_BACK_CM, POS_LIMIT_FORWARD_CM),
            ],
            last_frame: None,
            last_seen_seq: 0,
        }
    }
}

static PIPELINE: Mutex<Pipeline> = Mutex::new(Pipeline::new());

/// Smoothed pose returned by `tick_frame`. Both tuples are in the same
/// units as the underlying atomics: rotation in degrees, position in
/// body-frame centimetres `(right, up, forward)`.
#[derive(Debug, Clone, Copy)]
pub struct SmoothedPose {
    pub rotation: (f64, f64, f64),
    pub position: (f64, f64, f64),
}

/// Tick the pipeline once per render frame. Reads raw atomics, advances
/// the interpolator + smoother, writes the smoothed result to
/// `ATOMIC_SMOOTHED_ROTATION` / `ATOMIC_SMOOTHED_POSITION` so the D3D
/// overlay can read them, and returns the same values for the engine
/// hook to consume directly.
///
/// Safe to call multiple times per wall-clock frame (shadow / reflection
/// passes that re-trigger the camera hook). Each call advances
/// interpolator progress by the wall-clock dt since the previous call,
/// so total progress across N calls equals one frame.
pub fn tick_frame() -> SmoothedPose {
    let mut pipe = PIPELINE.lock();

    let now = Instant::now();
    let dt = match pipe.last_frame {
        Some(prev) => (now - prev).as_secs_f64().clamp(MIN_FRAME_DT, MAX_FRAME_DT),
        None => FIRST_FRAME_DT,
    };
    pipe.last_frame = Some(now);

    let seq = ATOMIC_SAMPLE_SEQ.load(Ordering::Acquire);
    let is_new = seq != pipe.last_seen_seq;
    pipe.last_seen_seq = seq;

    // Re-read the connection locality every frame so switching between a
    // local OpenTrack instance and a phone on WiFi picks up the other
    // parameter without a game restart.
    let smoothing = get_effective_smoothing(
        crate::config::local_smoothing(),
        crate::config::remote_smoothing(),
        crate::opentrack::is_remote_connection(),
    );

    let (raw_yaw, raw_pitch, raw_roll) = get_recentered_rotation_atomic();
    let iy = pipe.rot[0].update(raw_yaw, is_new, dt);
    let ip = pipe.rot[1].update(raw_pitch, is_new, dt);
    let ir = pipe.rot[2].update(raw_roll, is_new, dt);
    let sy = pipe.rot_smooth[0].update(iy, smoothing, dt);
    let sp = pipe.rot_smooth[1].update(ip, smoothing, dt);
    let sr = pipe.rot_smooth[2].update(ir, smoothing, dt);
    ATOMIC_SMOOTHED_ROTATION.store(sy, sp, sr);

    let (raw_x, raw_y_pos, raw_z) = get_recentered_position_atomic();
    let sx = pipe.pos[0].update(raw_x, is_new, dt, smoothing);
    let sy_pos = pipe.pos[1].update(raw_y_pos, is_new, dt, smoothing);
    let sz = pipe.pos[2].update(raw_z, is_new, dt, smoothing);
    ATOMIC_SMOOTHED_POSITION.store(sx, sy_pos, sz);

    SmoothedPose {
        rotation: (sy, sp, sr),
        position: (sx, sy_pos, sz),
    }
}

/// Reset all interpolation + smoothing state. Called from the recenter
/// path so the new center doesn't lerp out from the old smoothed pose,
/// and from the tracking-toggle so a long disabled period doesn't leave
/// a giant dt waiting on the next tick.
pub fn reset() {
    let mut pipe = PIPELINE.lock();
    for i in 0..3 {
        pipe.rot[i].reset();
        pipe.rot_smooth[i].reset();
        pipe.pos[i].reset();
    }
    pipe.last_frame = None;
    pipe.last_seen_seq = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolator_first_sample_parks_at_value() {
        let mut interp = Interpolator::linear();
        let out = interp.update(42.0, true, 0.016);
        assert!((out - 42.0).abs() < 1e-9);
    }

    #[test]
    fn interpolator_lerps_between_samples() {
        // Tracker at 60Hz, display at 120Hz: every other frame is a
        // non-new-sample frame, and on the next-new-sample frame the
        // call lands halfway through the freshly-opened segment.
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, 0.0); // sample at t=0
        interp.update(0.0, false, 1.0 / 120.0); // no-new frame at t=8.33ms
        let mid = interp.update(10.0, true, 1.0 / 120.0); // sample at t=16.67ms
                                                          // sample_interval just became 1/60; progress=0 then += 0.5
        assert!(mid > 4.0 && mid < 6.0, "expected ~5, got {}", mid);
    }

    #[test]
    fn interpolator_lerps_within_open_segment() {
        // Two samples 1/60 apart; mid-segment 120Hz tick should be
        // halfway between from and to.
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, 0.0);
        interp.update(0.0, false, 1.0 / 120.0);
        interp.update(10.0, true, 1.0 / 120.0); // open segment 0->10
        let mid = interp.update(10.0, false, 1.0 / 120.0);
        // Was at 5.0 after the new sample; advance another half.
        assert!(mid > 9.0 && mid <= 10.5, "expected ~10, got {}", mid);
    }

    #[test]
    fn interpolator_extrapolation_capped() {
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, 0.0);
        interp.update(10.0, true, 1.0 / 60.0);
        // Drive far past the next expected sample with no new data
        let out = interp.update(10.0, false, 1.0);
        // Cap is 1.5 of the segment so output should be at most 15
        assert!(out <= 15.0 + 1e-6, "extrapolation not capped: {}", out);
    }

    #[test]
    fn thirty_hertz_first_segment_lands_on_its_sample() {
        // Two samples closer together than MIN_SAMPLE_INTERVAL teach the EMA
        // nothing - a shadow or reflection pass re-entering the camera hook
        // inside one frame with a packet in between - so the first real
        // segment runs on the default interval. For a 30Hz feed on a 120Hz
        // display that segment must land on the pose the tracker reported
        // after one sample period: guessing too fast reaches the
        // extrapolation cap halfway through and sits 50% past it, guessing
        // too slow leaves the camera short of a sample that already arrived.
        let frame = 1.0 / 120.0;
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, frame); // first sample parks here
        interp.update(10.0, true, MIN_FRAME_DT); // second hook pass, same frame

        let mut out = 0.0;
        for _ in 0..4 {
            out = interp.update(10.0, false, frame); // one 30Hz sample period
        }
        assert!(
            (out - 10.0).abs() < 0.5,
            "first segment missed its sample by more than 5%: {}",
            out
        );
    }

    #[test]
    fn extrapolation_holds_through_a_dropped_packet_burst() {
        // Under the hold threshold a stalled sample is just a dropped
        // packet: the prediction continues to the cap and stays there, the
        // same as it always did. Retreating here would drag the camera
        // back against a head that is still turning.
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, 0.0);
        interp.update(10.0, true, 1.0 / 60.0);

        let mut out = 0.0;
        for _ in 0..12 {
            out = interp.update(10.0, false, 1.0 / 60.0); // 200ms of silence
        }
        assert!(
            out >= 15.0 - 1e-6,
            "hold window regressed, extrapolation retreated early: {}",
            out
        );
    }

    #[test]
    fn extrapolation_expires_when_samples_stop() {
        // A tracker that has lost the face streams its last value forever.
        // Held at the cap, a 10 unit step renders as 15 and never comes
        // back; it must ease to the value the tracker actually reported.
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, 0.0);
        interp.update(10.0, true, 1.0 / 60.0);

        let mut out = 0.0;
        for _ in 0..60 {
            out = interp.update(10.0, false, 1.0 / 60.0); // 1s of silence
        }
        assert!(
            (out - 10.0).abs() < 1e-6,
            "extrapolation parked past the reported sample: {}",
            out
        );
    }

    #[test]
    fn segment_position_eases_on_the_wall_clock() {
        // Exactly at the hold threshold nothing has moved yet, the decay is
        // smoothstep so halfway through it is halfway back, and past it the
        // segment sits on the reported sample.
        let capped = 1.0 + MAX_EXTRAPOLATION_FRACTION;
        assert!((segment_position(capped, EXTRAPOLATION_HOLD) - capped).abs() < 1e-9);
        assert!(
            (segment_position(capped, EXTRAPOLATION_HOLD + EXTRAPOLATION_DECAY / 2.0) - 1.25).abs()
                < 1e-9
        );
        assert!(
            (segment_position(
                capped,
                EXTRAPOLATION_HOLD + EXTRAPOLATION_DECAY + EXTRAPOLATION_DECAY
            ) - 1.0)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn angle_helpers_wrap_at_the_seam() {
        assert!((normalize_angle(190.0) - (-170.0)).abs() < 1e-9);
        assert!((normalize_angle(-190.0) - 170.0).abs() < 1e-9);
        assert!((normalize_angle(45.0) - 45.0).abs() < 1e-9);
        // Both directions: a delta that only works one way round is a sign
        // error that half the tests would still pass.
        assert!((shortest_angle_delta(179.0, -179.0) - 2.0).abs() < 1e-9);
        assert!((shortest_angle_delta(-179.0, 179.0) - (-2.0)).abs() < 1e-9);
        assert!((shortest_angle_delta(10.0, 20.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn interpolator_crosses_the_seam_the_short_way() {
        // 179 to -179 is a 2 degree head movement. Lerping the raw scalar
        // difference sweeps -358 degrees the long way round instead, through
        // every heading in between. Tested in both directions, because a
        // sign error in the delta passes one and fails the other.
        let frame = 1.0 / 120.0;
        for &(from, to) in &[(179.0, -179.0), (-179.0, 179.0)] {
            let mut interp = Interpolator::angular();
            interp.update(from, true, frame); // parks here
            interp.update(from, false, frame); // measures a 2-frame sample interval
            let mid = interp.update(to, true, frame); // lands halfway across

            // Halfway through a 2 degree hop across the seam is +/-180.
            let error = shortest_angle_delta(180.0, mid).abs();
            assert!(
                error < 1.0,
                "seam crossing {} -> {} went the long way: midpoint {} is {} degrees off 180",
                from,
                to,
                mid,
                error
            );
        }
    }

    #[test]
    fn linear_axis_does_not_wrap_at_180() {
        // Position rides the same interpolator, in centimetres. Normalising
        // it would turn a 200cm reading into -160cm, throwing the camera to
        // the opposite side of the head's travel.
        let frame = 1.0 / 120.0;
        let mut interp = Interpolator::linear();
        interp.update(0.0, true, frame);
        let out = interp.update(200.0, true, frame);
        assert!(
            (out - 200.0).abs() < 1e-9,
            "a linear axis wrapped at the angular seam: {}",
            out
        );
    }

    #[test]
    fn smoother_crosses_the_seam_the_short_way() {
        // The smoother needs the shortest path as much as the interpolator:
        // seam-correct interpolation feeding a scalar-lerp smoother still
        // swings the camera the long way round.
        let dt = 1.0 / 60.0;
        for &(seed, target) in &[(179.0, -179.0), (-179.0, 179.0)] {
            let mut s = Smoother::angular();
            s.update(seed, DEFAULT_REMOTE_SMOOTHING, dt);
            let out = s.update(target, DEFAULT_REMOTE_SMOOTHING, dt);

            let moved = shortest_angle_delta(seed, out);
            let wanted = shortest_angle_delta(seed, target);
            assert!(
                moved * wanted > 0.0,
                "smoothing {} toward {} moved the wrong way: {}",
                seed,
                target,
                out
            );
            assert!(
                moved.abs() <= wanted.abs() + 1e-9,
                "smoothing {} toward {} overshot the 2 degree gap: {}",
                seed,
                target,
                out
            );
        }
    }

    #[test]
    fn smoother_first_value_is_target() {
        let mut s = Smoother::linear();
        let out = s.update(50.0, 0.0, 0.016);
        assert!((out - 50.0).abs() < 1e-9);
    }

    #[test]
    fn smoother_converges_toward_target() {
        let mut s = Smoother::linear();
        s.update(0.0, DEFAULT_REMOTE_SMOOTHING, 0.016);
        let mut last = 0.0;
        for _ in 0..30 {
            last = s.update(100.0, DEFAULT_REMOTE_SMOOTHING, 0.016);
        }
        assert!(last > 90.0, "smoother didn't converge: {}", last);
    }

    #[test]
    fn smoother_zero_smoothing_is_near_instant() {
        // LocalSmoothing defaults to 0.0: speed is the 50.0 end of the
        // ramp, so a single 16ms step lands almost on the target. There
        // is no floor holding it back any more.
        let mut s = Smoother::linear();
        s.update(0.0, DEFAULT_LOCAL_SMOOTHING, 0.016);
        let out = s.update(100.0, DEFAULT_LOCAL_SMOOTHING, 0.016);
        assert!(out > 54.0, "zero smoothing still floored: {}", out);
    }

    #[test]
    fn smoother_remote_smoothing_lags() {
        // RemoteSmoothing defaults to 0.15, which must visibly lag a
        // single step rather than snap.
        let mut s = Smoother::linear();
        s.update(0.0, DEFAULT_REMOTE_SMOOTHING, 0.016);
        let out = s.update(100.0, DEFAULT_REMOTE_SMOOTHING, 0.016);
        assert!(out < 100.0, "remote smoothing not applied: {}", out);
    }

    #[test]
    fn effective_smoothing_selects_on_connection() {
        assert!(
            (get_effective_smoothing(0.0, 0.15, true) - 0.15).abs() < 1e-9,
            "remote connection must use remote smoothing"
        );
        assert!(
            (get_effective_smoothing(0.0, 0.15, false) - 0.0).abs() < 1e-9,
            "local connection must use local smoothing"
        );
    }

    #[test]
    fn position_axis_holds_the_limits() {
        let mut axis = PositionAxis::new(-POS_LIMIT_BACK_CM, POS_LIMIT_FORWARD_CM);
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            axis.update(100.0, true, dt, DEFAULT_REMOTE_SMOOTHING);
        }
        let out = axis.update(100.0, true, dt, DEFAULT_REMOTE_SMOOTHING);
        assert!(
            out <= POS_LIMIT_FORWARD_CM + 1e-9,
            "forward lean exceeded its limit: {}",
            out
        );

        for _ in 0..60 {
            axis.update(-100.0, true, dt, DEFAULT_REMOTE_SMOOTHING);
        }
        let out = axis.update(-100.0, true, dt, DEFAULT_REMOTE_SMOOTHING);
        assert!(
            out >= -POS_LIMIT_BACK_CM - 1e-9,
            "backward lean exceeded its tighter limit: {}",
            out
        );
    }

    #[test]
    fn position_axis_does_not_wind_up_outside_its_limits() {
        // Hold a 100cm lean, well past the 40cm forward limit, then return
        // the head to centre. Smoothing an unclamped input leaves the state
        // parked at 100 and it stays saturated on the way back, pinning the
        // output at the limit long after the head has returned.
        let mut axis = PositionAxis::new(-POS_LIMIT_BACK_CM, POS_LIMIT_FORWARD_CM);
        let dt = 1.0 / 60.0;
        for _ in 0..60 {
            axis.update(100.0, true, dt, DEFAULT_REMOTE_SMOOTHING);
        }

        let out = axis.update(0.0, true, dt, DEFAULT_REMOTE_SMOOTHING);
        assert!(
            out < POS_LIMIT_FORWARD_CM - 1.0,
            "smoothing state wound up outside the limit and pinned the output: {}",
            out
        );
    }

    #[test]
    fn reset_clears_state() {
        // Drive the pipeline forward then reset and confirm next tick
        // parks at the new raw values without lerping from the old.
        super::reset();
        super::tick_frame();
        super::reset();
        let pipe = PIPELINE.lock();
        assert!(pipe.last_frame.is_none());
        assert_eq!(pipe.last_seen_seq, 0);
    }
}
