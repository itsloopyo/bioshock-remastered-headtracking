//! Conformance harness for `cameraunlock-core/data/pipeline-conformance.json`.
//!
//! The core ships the porting doctrine's checks as vectors and a runner that
//! owns every assertion; a port supplies only this - a dumb executor that reads
//! a line-oriented command stream on stdin and prints one line of numbers per
//! step. See `cameraunlock-core/scripts/pipeline-vectors/run-vectors.mjs` for
//! the protocol, and `pixi run test-vectors` for the wiring.
//!
//! Everything this file does NOT implement is answered with an explicit
//! `skip <reason>`, which the runner reports separately from a pass. Staying
//! silent about a unit we lack, or ignoring a configuration key we do not have a
//! setting for, would report a pass for a test that never ran.

use std::io::{self, BufRead, Write};

use crate::smoothing::{
    get_effective_smoothing, Interpolator, Smoother, DEFAULT_LOCAL_SMOOTHING,
    DEFAULT_REMOTE_SMOOTHING,
};

/// What this port can be driven through, and what it cannot.
enum Unit {
    PoseInterpolator([Interpolator; 3]),
    PositionInterpolator([Interpolator; 3]),
    TrackingProcessor {
        smoothers: [Smoother; 3],
        smoothing: f64,
    },
    Packet,
    /// The whole per-frame pipeline, driven at a chosen dt. Datagrams are fed
    /// through the same decode and atomic writes the receive loop uses, so this
    /// exercises the duplicate-sample filter end to end.
    SessionRot,
    Skipped,
}

struct Harness {
    unit: Unit,
    out: io::Stdout,
}

fn from_hex(hex: &str) -> Vec<u8> {
    (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("bad hex"))
        .collect()
}

fn parse(token: Option<&&str>) -> f64 {
    token
        .expect("harness command is missing an argument")
        .parse()
        .expect("harness argument is not a number")
}

impl Harness {
    fn new() -> Self {
        Self {
            unit: Unit::Skipped,
            out: io::stdout(),
        }
    }

    fn emit(&mut self, values: &[f64]) {
        let line: Vec<String> = values.iter().map(|v| format!("{v:.10}")).collect();
        writeln!(self.out, "{}", line.join(" ")).expect("stdout closed");
    }

    fn skip(&mut self, reason: &str) {
        self.unit = Unit::Skipped;
        writeln!(self.out, "skip {reason}").expect("stdout closed");
    }

    fn ok(&mut self) {
        writeln!(self.out, "ok").expect("stdout closed");
    }

    /// `begin`: decide whether this vector can run at all, and say why not.
    fn configure(&mut self, unit_name: &str, vector_id: &str, cfg: &[(String, f64)]) {
        // A vector this port has no mechanism for, named individually because the
        // unit itself is otherwise supported.
        if vector_id == "wire-position-is-centimetres" {
            self.skip("position is carried in centimetres end to end (POS_LIMIT_*_CM), so there is no cm-to-metres conversion in this port to check");
            return;
        }

        let allowed: &[&str] = match unit_name {
            "pose_interpolator" | "position_interpolator" | "packet" => &[],
            "tracking_processor" => &["local_smoothing", "remote_smoothing", "is_remote"],
            "position_processor" => {
                self.skip("the position pipeline is centimetres with mod-specific limits and has no sensitivity, inversion or pivot stage, so the vector's settings do not map onto it");
                return;
            }
            "euler_roundtrip" => {
                self.skip("the port has no orientation compose/decompose - it consumes Euler from the wire and applies Euler to the engine");
                return;
            }
            "session_rot" => {
                crate::smoothing::reset();
                &[]
            }
            "session_pos" => {
                self.skip("the session's position half is centimetres with mod-specific limits, so the vector's metres do not map onto it");
                return;
            }
            other => {
                self.skip(&format!("harness does not implement unit {other}"));
                return;
            }
        };

        for (key, _) in cfg {
            if !allowed.contains(&key.as_str()) {
                self.skip(&format!("{unit_name} has no setting for cfg key {key}"));
                return;
            }
        }

        let get = |name: &str, fallback: f64| {
            cfg.iter()
                .find(|(k, _)| k == name)
                .map_or(fallback, |(_, v)| *v)
        };

        self.unit = match unit_name {
            // Yaw and roll wrap at the seam, pitch does not. Same ordering as
            // Pipeline::new, so the vectors exercise the real axis kinds.
            "pose_interpolator" => Unit::PoseInterpolator([
                Interpolator::angular(),
                Interpolator::linear(),
                Interpolator::angular(),
            ]),
            "position_interpolator" => Unit::PositionInterpolator([
                Interpolator::linear(),
                Interpolator::linear(),
                Interpolator::linear(),
            ]),
            "tracking_processor" => Unit::TrackingProcessor {
                smoothers: [Smoother::angular(), Smoother::linear(), Smoother::angular()],
                smoothing: get_effective_smoothing(
                    get("local_smoothing", DEFAULT_LOCAL_SMOOTHING),
                    get("remote_smoothing", DEFAULT_REMOTE_SMOOTHING),
                    get("is_remote", 0.0) != 0.0,
                ),
            },
            "packet" => Unit::Packet,
            "session_rot" => Unit::SessionRot,
            _ => unreachable!("unit filtered above"),
        };
        self.ok();
    }

    fn step(&mut self, args: &[&str]) {
        match &mut self.unit {
            Unit::PoseInterpolator(axes) | Unit::PositionInterpolator(axes) => {
                let is_new = args[3] != "0";
                let dt = parse(args.get(4));
                let out = [
                    axes[0].update(parse(args.first()), is_new, dt),
                    axes[1].update(parse(args.get(1)), is_new, dt),
                    axes[2].update(parse(args.get(2)), is_new, dt),
                ];
                self.emit(&out);
            }
            Unit::TrackingProcessor {
                smoothers,
                smoothing,
            } => {
                let smoothing = *smoothing;
                let dt = parse(args.get(3));
                let out = [
                    smoothers[0].update(parse(args.first()), smoothing, dt),
                    smoothers[1].update(parse(args.get(1)), smoothing, dt),
                    smoothers[2].update(parse(args.get(2)), smoothing, dt),
                ];
                self.emit(&out);
            }
            Unit::Packet | Unit::SessionRot | Unit::Skipped => {
                panic!("unit does not take an `s` step")
            }
        }
    }

    /// One render frame of the whole pipeline. A datagram, when present, goes
    /// through `decode_datagram` and the same atomic writes the receive loop
    /// performs, sequence bump included - so a stream of bit-identical packets
    /// reaches the pipeline exactly as it does in the game.
    fn session_frame(&mut self, hex: Option<&str>, dt: f64) {
        if let Some(hex) = hex {
            if let Some(d) = crate::opentrack::decode_datagram(&from_hex(hex)) {
                crate::tracking::update_rotation_atomic(d.yaw, d.pitch, d.roll);
                crate::tracking::update_position_atomic(d.x, d.y, d.z);
                crate::tracking::ATOMIC_SAMPLE_SEQ
                    .fetch_add(1, std::sync::atomic::Ordering::Release);
            }
        }
        let pose = crate::smoothing::tick_with_dt(dt);
        self.emit(&[pose.rotation.0, pose.rotation.1, pose.rotation.2]);
    }

    fn packet(&mut self, hex: &str) {
        let bytes = from_hex(hex);

        // The port keeps position in centimetres, the wire's unit, and the
        // runner's position columns are metres, so they are written divided by
        // 100. That division is the harness's, which is why
        // wire-position-is-centimetres is skipped above rather than passed by it.
        // The port accepts or drops a datagram whole, so ok_rotation and
        // ok_position are both ok.
        let (trailer_present, counter) = match crate::opentrack::hcam_trailer(&bytes) {
            Some(counter) => (1.0, f64::from(counter)),
            None => (0.0, 0.0),
        };
        let row = match crate::opentrack::decode_datagram(&bytes) {
            Some(d) => [
                1.0,
                d.yaw,
                d.pitch,
                d.roll,
                d.x / 100.0,
                d.y / 100.0,
                d.z / 100.0,
                trailer_present,
                counter,
                1.0,
                1.0,
            ],
            None => [0.0; 11],
        };
        self.emit(&row);
    }
}

/// Read the command stream on stdin and answer it. Returns when `bye` arrives
/// or stdin closes.
pub fn run() {
    let stdin = io::stdin();
    let mut harness = Harness::new();
    let mut unit_name = String::new();
    let mut vector_id = String::new();
    let mut cfg: Vec<(String, f64)> = Vec::new();

    for line in stdin.lock().lines() {
        let line = line.expect("stdin read failed");
        let mut parts = line.split_whitespace();
        let Some(cmd) = parts.next() else { continue };
        let args: Vec<&str> = parts.collect();

        match cmd {
            "unit" => {
                unit_name = args[0].to_string();
                vector_id = args[1].to_string();
                cfg.clear();
                harness.unit = Unit::Skipped;
            }
            "cfg" => cfg.push((args[0].to_string(), parse(args.get(1)))),
            "begin" => harness.configure(&unit_name, &vector_id, &cfg),
            "s" => {
                if !matches!(harness.unit, Unit::Skipped) {
                    harness.step(&args);
                }
            }
            "q" => {
                if !matches!(harness.unit, Unit::Skipped) {
                    harness.packet(args[0]);
                }
            }
            "p" => {
                if !matches!(harness.unit, Unit::Skipped) {
                    harness.session_frame(Some(args[0]), parse(args.get(1)));
                }
            }
            "f" => {
                if !matches!(harness.unit, Unit::Skipped) {
                    harness.session_frame(None, parse(args.first()));
                }
            }
            "e" => {
                assert!(
                    matches!(harness.unit, Unit::Skipped),
                    "harness accepted euler_roundtrip, which this port has no code for"
                );
            }
            "end" => {}
            "bye" => break,
            other => panic!("unknown harness command: {other}"),
        }
    }

    harness.out.flush().expect("stdout closed");
}
