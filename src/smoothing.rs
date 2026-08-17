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
const DEFAULT_SAMPLE_INTERVAL: f64 = 1.0 / 60.0;
const MIN_SAMPLE_INTERVAL: f64 = 0.001;
const MAX_SAMPLE_INTERVAL: f64 = 0.2;
const MAX_EXTRAPOLATION_FRACTION: f64 = 0.5;

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

/// Lower clamp on per-frame dt. Prevents division-by-near-zero in the
/// progress integration if two ticks land in the same microsecond.
const MIN_FRAME_DT: f64 = 0.0001;

/// Upper clamp on per-frame dt. Caps catch-up after a stall (alt-tab,
/// pause, debug breakpoint) so the interpolator can't fling itself
/// past the latest sample.
const MAX_FRAME_DT: f64 = 0.1;

#[derive(Debug, Clone, Copy)]
struct Interpolator {
    from: f64,
    to: f64,
    progress: f64,
    sample_interval: f64,
    time_since_last_sample: f64,
    has_first_sample: bool,
    has_second_sample: bool,
}

impl Interpolator {
    const fn new() -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            progress: 0.0,
            sample_interval: DEFAULT_SAMPLE_INTERVAL,
            time_since_last_sample: 0.0,
            has_first_sample: false,
            has_second_sample: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
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
            self.from += (self.to - self.from) * t;

            self.to = raw;
            self.progress = 0.0;
            self.time_since_last_sample = 0.0;
        }

        if !self.has_first_sample {
            return raw;
        }

        self.progress += dt / self.sample_interval;

        let pt = segment_position(self.progress, self.time_since_last_sample);
        self.from + (self.to - self.from) * pt
    }
}

#[derive(Debug, Clone, Copy)]
struct Smoother {
    current: f64,
    has_value: bool,
}

impl Smoother {
    const fn new() -> Self {
        Self {
            current: 0.0,
            has_value: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn update(&mut self, target: f64, smoothing: f64, dt: f64) -> f64 {
        if !self.has_value {
            self.current = target;
            self.has_value = true;
            return target;
        }
        // 0..1 maps to speeds 50..0.1. Matches SmoothingUtils.cs / .h.
        let speed = lerp(50.0, 0.1, smoothing).clamp(0.1, 50.0);
        let t = 1.0 - (-speed * dt).exp();
        self.current += (target - self.current) * t;
        self.current
    }
}

#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

struct Pipeline {
    rot: [Interpolator; 3],
    pos: [Interpolator; 3],
    rot_smooth: [Smoother; 3],
    pos_smooth: [Smoother; 3],
    last_frame: Option<Instant>,
    last_seen_seq: u64,
}

impl Pipeline {
    const fn new() -> Self {
        Self {
            rot: [Interpolator::new(); 3],
            pos: [Interpolator::new(); 3],
            rot_smooth: [Smoother::new(); 3],
            pos_smooth: [Smoother::new(); 3],
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
        None => DEFAULT_SAMPLE_INTERVAL,
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
    let ix = pipe.pos[0].update(raw_x, is_new, dt);
    let iy_pos = pipe.pos[1].update(raw_y_pos, is_new, dt);
    let iz = pipe.pos[2].update(raw_z, is_new, dt);
    let sx = pipe.pos_smooth[0].update(ix, smoothing, dt);
    let sy_pos = pipe.pos_smooth[1].update(iy_pos, smoothing, dt);
    let sz = pipe.pos_smooth[2].update(iz, smoothing, dt);
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
        pipe.pos[i].reset();
        pipe.rot_smooth[i].reset();
        pipe.pos_smooth[i].reset();
    }
    pipe.last_frame = None;
    pipe.last_seen_seq = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolator_first_sample_parks_at_value() {
        let mut interp = Interpolator::new();
        let out = interp.update(42.0, true, 0.016);
        assert!((out - 42.0).abs() < 1e-9);
    }

    #[test]
    fn interpolator_lerps_between_samples() {
        // Tracker at 60Hz, display at 120Hz: every other frame is a
        // non-new-sample frame, and on the next-new-sample frame the
        // call lands halfway through the freshly-opened segment.
        let mut interp = Interpolator::new();
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
        let mut interp = Interpolator::new();
        interp.update(0.0, true, 0.0);
        interp.update(0.0, false, 1.0 / 120.0);
        interp.update(10.0, true, 1.0 / 120.0); // open segment 0->10
        let mid = interp.update(10.0, false, 1.0 / 120.0);
        // Was at 5.0 after the new sample; advance another half.
        assert!(mid > 9.0 && mid <= 10.5, "expected ~10, got {}", mid);
    }

    #[test]
    fn interpolator_extrapolation_capped() {
        let mut interp = Interpolator::new();
        interp.update(0.0, true, 0.0);
        interp.update(10.0, true, 1.0 / 60.0);
        // Drive far past the next expected sample with no new data
        let out = interp.update(10.0, false, 1.0);
        // Cap is 1.5 of the segment so output should be at most 15
        assert!(out <= 15.0 + 1e-6, "extrapolation not capped: {}", out);
    }

    #[test]
    fn extrapolation_holds_through_a_dropped_packet_burst() {
        // Under the hold threshold a stalled sample is just a dropped
        // packet: the prediction continues to the cap and stays there, the
        // same as it always did. Retreating here would drag the camera
        // back against a head that is still turning.
        let mut interp = Interpolator::new();
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
        let mut interp = Interpolator::new();
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
    fn smoother_first_value_is_target() {
        let mut s = Smoother::new();
        let out = s.update(50.0, 0.0, 0.016);
        assert!((out - 50.0).abs() < 1e-9);
    }

    #[test]
    fn smoother_converges_toward_target() {
        let mut s = Smoother::new();
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
        let mut s = Smoother::new();
        s.update(0.0, DEFAULT_LOCAL_SMOOTHING, 0.016);
        let out = s.update(100.0, DEFAULT_LOCAL_SMOOTHING, 0.016);
        assert!(out > 54.0, "zero smoothing still floored: {}", out);
    }

    #[test]
    fn smoother_remote_smoothing_lags() {
        // RemoteSmoothing defaults to 0.15, which must visibly lag a
        // single step rather than snap.
        let mut s = Smoother::new();
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
