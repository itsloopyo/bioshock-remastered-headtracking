//! OpenTrack UDP protocol handler
//!
//! Receives 3DOF head tracking data from OpenTrack via UDP on port 4242.
//! The OpenTrack UDP protocol sends 48 bytes containing 6 IEEE 754
//! little-endian doubles: x, y, z, yaw, pitch, roll.
//! We only use yaw, pitch, roll for 3DOF tracking.
//!
//! # Protocol Details
//!
//! OpenTrack sends UDP datagrams at approximately 250Hz containing:
//! - Bytes 0-7: X position (centimeters) as IEEE 754 little-endian double
//! - Bytes 8-15: Y position (centimeters) as IEEE 754 little-endian double
//! - Bytes 16-23: Z position (centimeters) as IEEE 754 little-endian double
//! - Bytes 24-31: Yaw rotation (degrees) as IEEE 754 little-endian double
//! - Bytes 32-39: Pitch rotation (degrees) as IEEE 754 little-endian double
//! - Bytes 40-47: Roll rotation (degrees) as IEEE 754 little-endian double
//!
//! For 3DOF head tracking, we only use yaw, pitch, and roll (ignoring position).

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::thread;
use std::time::{Duration, Instant};

use std::sync::atomic::{AtomicBool, Ordering};

use crate::tracking::{
    update_position_atomic, update_rotation_atomic, ATOMIC_SAMPLE_SEQ, GLOBAL_STATE,
};

/// OpenTrack UDP port (project-standard default).
pub const OPENTRACK_PORT: u16 = 4242;

/// OpenTrack packet size: 6 doubles * 8 bytes = 48 bytes
pub const PACKET_SIZE: usize = 48;

const RECEIVE_BUFFER_SIZE: usize = 64;
const TRAILER_SIZE: usize = 54;
const TRAILER_MAGIC: &[u8; 4] = b"HCAM";
const TRAILER_VERSION: u8 = 1;
/// Packet silence after which trailer first-sighting re-arms. A tracker
/// app restart resets its counter, so a value latched from the old
/// session would swallow the first CENTER press of the new one.
///
/// Fixed at ~5s by the wire contract, and deliberately far longer than a
/// connection-liveness timeout: at 500ms an ordinary Wi-Fi stall inside a
/// recenter burst re-armed mid-burst, so the burst's tail - carrying the
/// SAME counter - read as a second press and recentred on whatever pose
/// the head had drifted to. Matches `kRecenterRearmMs` in the core's
/// `UdpReceiver` / `PollingUdpReceiver`.
const RECENTER_REARM: Duration = Duration::from_millis(5000);
static RECENTER_REQUESTED: AtomicBool = AtomicBool::new(false);

/// True when the most recent packet came from off-box. Set from the
/// datagram's sender address on every packet, so switching between a
/// local OpenTrack instance and a phone on WiFi re-selects the smoothing
/// parameter without a game restart. Starts `false`: before any packet
/// arrives there is no connection to smooth.
static IS_REMOTE_CONNECTION: AtomicBool = AtomicBool::new(false);

/// Socket read timeout in milliseconds (4ms allows ~250Hz polling)
const READ_TIMEOUT_MS: u64 = 4;

/// Bind retry cadence when the port is held by another process. Mirrors
/// `OpenTrackReceiver` in cameraunlock-core/csharp so users get the same
/// "close the conflicting tracker, tracking comes back" experience.
const BIND_RETRY_INTERVAL_MS: u64 = 5000;
const BIND_RETRY_LOG_INTERVAL_MS: u64 = 30000;

/// Parsed OpenTrack data packet
///
/// Contains the full 6DOF tracking data from OpenTrack, though this mod
/// only uses the rotation components (yaw, pitch, roll) for 3DOF tracking.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenTrackData {
    /// X position in centimeters - IGNORED for 3DOF
    pub x: f64,
    /// Y position in centimeters - IGNORED for 3DOF
    pub y: f64,
    /// Z position in centimeters - IGNORED for 3DOF
    pub z: f64,
    /// Yaw rotation in degrees (horizontal head turn) - APPLIED
    pub yaw: f64,
    /// Pitch rotation in degrees (vertical head tilt) - APPLIED
    pub pitch: f64,
    /// Roll rotation in degrees (head tilt side-to-side) - APPLIED
    pub roll: f64,
}

impl OpenTrackData {
    /// Parse a 48-byte packet into OpenTrackData
    ///
    /// OpenTrack sends 6 IEEE 754 little-endian doubles in order:
    /// x, y, z, yaw, pitch, roll
    ///
    /// # Arguments
    /// * `data` - Exactly 48 bytes of UDP packet data
    ///
    /// # Returns
    /// Parsed OpenTrackData with all 6 values extracted
    pub fn from_bytes(data: &[u8; PACKET_SIZE]) -> Self {
        Self {
            x: f64::from_le_bytes(data[0..8].try_into().unwrap()),
            y: f64::from_le_bytes(data[8..16].try_into().unwrap()),
            z: f64::from_le_bytes(data[16..24].try_into().unwrap()),
            yaw: f64::from_le_bytes(data[24..32].try_into().unwrap()),
            pitch: f64::from_le_bytes(data[32..40].try_into().unwrap()),
            roll: f64::from_le_bytes(data[40..48].try_into().unwrap()),
        }
    }

    /// True only when every field is a finite number.
    ///
    /// The receiver binds `0.0.0.0`, so any host on the network (or a
    /// glitching tracker) can deliver a datagram. A single non-finite value
    /// would flow into the exponential smoother and pin its running value
    /// at `NaN` permanently, so every later sample stays `NaN` until a
    /// recenter or toggle resets the pipeline. Non-finite tracking data is
    /// never legitimate, so we drop the packet at the boundary rather than
    /// let it poison state.
    pub fn is_finite(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.z.is_finite()
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && self.roll.is_finite()
    }

    /// Create a 48-byte packet from OpenTrackData
    ///
    /// Useful for testing - creates a packet in the OpenTrack format
    /// that can be sent via UDP.
    ///
    /// # Returns
    /// 48-byte array containing the packet data
    #[cfg(test)]
    pub fn to_bytes(&self) -> [u8; PACKET_SIZE] {
        let mut buf = [0u8; PACKET_SIZE];
        buf[0..8].copy_from_slice(&self.x.to_le_bytes());
        buf[8..16].copy_from_slice(&self.y.to_le_bytes());
        buf[16..24].copy_from_slice(&self.z.to_le_bytes());
        buf[24..32].copy_from_slice(&self.yaw.to_le_bytes());
        buf[32..40].copy_from_slice(&self.pitch.to_le_bytes());
        buf[40..48].copy_from_slice(&self.roll.to_le_bytes());
        buf
    }
}

fn parse_recenter_counter(data: &[u8]) -> Option<u8> {
    if data.len() < TRAILER_SIZE
        || &data[48..52] != TRAILER_MAGIC
        || data[52] < TRAILER_VERSION
    {
        return None;
    }
    Some(data[53])
}

/// Tracks the Headcam trailer's recenter counter across datagrams so a
/// CENTER press is recognised exactly once: the first trailer sighting is
/// a press, a counter change is a press, a repeat is not.
struct RecenterDetector {
    last_counter: Option<u8>,
    last_packet_at: Option<Instant>,
}

impl RecenterDetector {
    const fn new() -> Self {
        Self {
            last_counter: None,
            last_packet_at: None,
        }
    }

    /// True when this datagram carries a press.
    fn observe(&mut self, datagram: &[u8], now: Instant) -> bool {
        if let Some(previous) = self.last_packet_at {
            if now.duration_since(previous) >= RECENTER_REARM {
                self.last_counter = None;
            }
        }
        self.last_packet_at = Some(now);

        let Some(counter) = parse_recenter_counter(datagram) else {
            return false;
        };
        let pressed = self.last_counter != Some(counter);
        self.last_counter = Some(counter);
        pressed
    }
}

/// Decode one datagram into a pose plus whether it carried a CENTER
/// press. `None` discards the datagram whole - and with it any press it
/// announced, because a press is only meaningful alongside the zeroed
/// pose the tracker sent it with.
fn decode_datagram(
    detector: &mut RecenterDetector,
    datagram: &[u8],
    now: Instant,
) -> Option<(OpenTrackData, bool)> {
    let pressed = detector.observe(datagram, now);

    let packet: &[u8; PACKET_SIZE] = datagram[..PACKET_SIZE].try_into().unwrap();
    let data = OpenTrackData::from_bytes(packet);

    // Drop non-finite packets at the boundary: a single NaN/Inf would pin
    // the smoother at NaN for the rest of the session.
    if !data.is_finite() {
        log::warn!("Discarding OpenTrack packet with non-finite values");
        return None;
    }

    Some((data, pressed))
}

pub fn try_consume_recenter_request() -> bool {
    RECENTER_REQUESTED.swap(false, Ordering::AcqRel)
}

/// True when tracking data is arriving from a remote network device
/// rather than from this machine. Mirrors
/// `OpenTrackReceiver.IsRemoteConnection` in the C# core; drives the
/// LocalSmoothing / RemoteSmoothing selection.
pub fn is_remote_connection() -> bool {
    IS_REMOTE_CONNECTION.load(Ordering::Acquire)
}

/// A sender is local when it is loopback (`127.0.0.1`, `::1`), and
/// remote otherwise. Same rule as `!IPAddress.IsLoopback(senderAddress)`
/// in the C# core.
fn is_remote_address(addr: &SocketAddr) -> bool {
    !addr.ip().is_loopback()
}

/// Spawn the OpenTrack UDP receiver thread.
///
/// The thread first tries to bind `0.0.0.0:4242`. If another process is
/// holding the port, it retries every 5s (logging every 30s) until either
/// the bind succeeds or shutdown is requested. The rest of the mod
/// (engine hook, hotkey poller, D3D overlay) keeps running through the
/// retry, so closing the conflicting tracker brings head tracking back
/// to life with no game restart. Binding to all interfaces lets
/// phone-based trackers send directly without an OpenTrack relay on the
/// PC.
pub fn start_receiver() {
    thread::spawn(|| {
        let Some(socket) = bind_with_retry() else {
            return;
        };
        if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS))) {
            log::error!("Failed to set OpenTrack socket read timeout: {}", e);
            return;
        }
        receive_loop(socket);
    });
}

fn bind_with_retry() -> Option<UdpSocket> {
    let addr = format!("0.0.0.0:{}", OPENTRACK_PORT);

    match UdpSocket::bind(&addr) {
        Ok(socket) => {
            log::info!("OpenTrack receiver started on port {}", OPENTRACK_PORT);
            return Some(socket);
        }
        Err(e) => {
            log::error!(
                "Failed to bind UDP port {} ({}) -- will retry every {}s",
                OPENTRACK_PORT,
                e,
                BIND_RETRY_INTERVAL_MS / 1000
            );
        }
    }

    let attempts_per_log = BIND_RETRY_LOG_INTERVAL_MS / BIND_RETRY_INTERVAL_MS;
    let mut attempts: u64 = 0;
    loop {
        // Sleep in 100ms slices so shutdown_requested is honoured promptly.
        for _ in 0..(BIND_RETRY_INTERVAL_MS / 100) {
            if GLOBAL_STATE.read().shutdown_requested {
                return None;
            }
            thread::sleep(Duration::from_millis(100));
        }

        attempts += 1;
        match UdpSocket::bind(&addr) {
            Ok(socket) => {
                log::info!(
                    "Bound UDP port {} after {} retries",
                    OPENTRACK_PORT,
                    attempts
                );
                return Some(socket);
            }
            Err(_) => {
                if attempts.is_multiple_of(attempts_per_log) {
                    log::warn!(
                        "Still waiting for UDP port {} ({}s elapsed)",
                        OPENTRACK_PORT,
                        attempts * BIND_RETRY_INTERVAL_MS / 1000
                    );
                }
            }
        }
    }
}

fn receive_loop(socket: UdpSocket) {
    let mut buf = [0u8; RECEIVE_BUFFER_SIZE];
    let mut recenter = RecenterDetector::new();

    loop {
        if GLOBAL_STATE.read().shutdown_requested {
            log::info!("OpenTrack receiver shutting down");
            break;
        }

        match socket.recv_from(&mut buf) {
            Ok((size, sender)) if size >= PACKET_SIZE => {
                IS_REMOTE_CONNECTION.store(is_remote_address(&sender), Ordering::Release);

                let Some((data, pressed)) =
                    decode_datagram(&mut recenter, &buf[..size], Instant::now())
                else {
                    continue;
                };

                // Update rotation + position using lock-free atomics
                // (optimized hot path).
                update_rotation_atomic(data.yaw, data.pitch, data.roll);
                update_position_atomic(data.x, data.y, data.z);
                // Bump the sequence counter AFTER the value writes.
                // Release ordering pairs with the Acquire load on the
                // render thread so the new yaw/pitch/roll are
                // guaranteed visible to the smoothing pipeline once it
                // observes the new sequence number.
                ATOMIC_SAMPLE_SEQ.fetch_add(1, Ordering::Release);

                // Also update GLOBAL_STATE for legacy compatibility
                // This is less frequent than reads, so RwLock overhead is acceptable
                {
                    let mut state = GLOBAL_STATE.write();
                    state.yaw = data.yaw;
                    state.pitch = data.pitch;
                    state.roll = data.roll;
                }

                // Published after the pose, never before it: the recenter
                // path captures whatever pose is current when it observes
                // the request, and the pose this packet carries is the one
                // the tracker just zeroed. Flagged first, a consumer that
                // read the request in the gap centred on the PREVIOUS,
                // pre-press pose and the view parked at that drift.
                if pressed {
                    RECENTER_REQUESTED.store(true, Ordering::Release);
                }
            }
            Ok((size, _)) => {
                log::warn!("Received packet with unexpected size: {} bytes", size);
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // Timeout, no data available - this is normal
            }
            Err(ref e) if e.kind() == io::ErrorKind::TimedOut => {
                // Timeout, no data available - this is normal
            }
            Err(e) => {
                log::error!("UDP receive error: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::time::Duration;

    #[test]
    fn loopback_sender_is_local() {
        assert!(!is_remote_address(&"127.0.0.1:4242".parse().unwrap()));
        assert!(!is_remote_address(&"[::1]:4242".parse().unwrap()));
    }

    #[test]
    fn lan_sender_is_remote() {
        assert!(is_remote_address(&"192.168.1.50:4242".parse().unwrap()));
    }

    /// Helper to compare f64 values with tolerance
    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-10
    }

    #[test]
    fn test_parse_zeros() {
        let buf = [0u8; PACKET_SIZE];
        let data = OpenTrackData::from_bytes(&buf);
        assert_eq!(data.x, 0.0);
        assert_eq!(data.y, 0.0);
        assert_eq!(data.z, 0.0);
        assert_eq!(data.yaw, 0.0);
        assert_eq!(data.pitch, 0.0);
        assert_eq!(data.roll, 0.0);
    }

    #[test]
    fn test_parse_known_values() {
        // 45.0 as f64 little-endian bytes
        let forty_five_bytes: [u8; 8] = 45.0_f64.to_le_bytes();

        let mut buf = [0u8; PACKET_SIZE];
        // Put 45.0 in yaw position (bytes 24-32)
        buf[24..32].copy_from_slice(&forty_five_bytes);

        let data = OpenTrackData::from_bytes(&buf);
        assert!(approx_eq(data.yaw, 45.0));
    }

    #[test]
    fn test_parse_endianness_negative_values() {
        // Test with negative values to verify little-endian byte order
        let test_data = OpenTrackData {
            x: -10.5,
            y: -20.25,
            z: -30.125,
            yaw: -45.0,
            pitch: -15.5,
            roll: -7.25,
        };

        let bytes = test_data.to_bytes();
        let parsed = OpenTrackData::from_bytes(&bytes);

        assert!(approx_eq(parsed.x, test_data.x), "X mismatch");
        assert!(approx_eq(parsed.y, test_data.y), "Y mismatch");
        assert!(approx_eq(parsed.z, test_data.z), "Z mismatch");
        assert!(approx_eq(parsed.yaw, test_data.yaw), "Yaw mismatch");
        assert!(approx_eq(parsed.pitch, test_data.pitch), "Pitch mismatch");
        assert!(approx_eq(parsed.roll, test_data.roll), "Roll mismatch");
    }

    #[test]
    fn test_parse_all_fields() {
        // Test all fields with distinct values
        let test_data = OpenTrackData {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            yaw: 45.0,
            pitch: 30.0,
            roll: 15.0,
        };

        let bytes = test_data.to_bytes();
        let parsed = OpenTrackData::from_bytes(&bytes);

        assert!(approx_eq(parsed.x, 1.0));
        assert!(approx_eq(parsed.y, 2.0));
        assert!(approx_eq(parsed.z, 3.0));
        assert!(approx_eq(parsed.yaw, 45.0));
        assert!(approx_eq(parsed.pitch, 30.0));
        assert!(approx_eq(parsed.roll, 15.0));
    }

    #[test]
    fn test_parse_extreme_values() {
        // Test with extreme but valid rotation values
        let test_data = OpenTrackData {
            x: 1000.0,
            y: -1000.0,
            z: 500.0,
            yaw: 180.0,   // Full turn
            pitch: 90.0,  // Looking straight up
            roll: -180.0, // Upside down
        };

        let bytes = test_data.to_bytes();
        let parsed = OpenTrackData::from_bytes(&bytes);

        assert!(approx_eq(parsed.yaw, 180.0));
        assert!(approx_eq(parsed.pitch, 90.0));
        assert!(approx_eq(parsed.roll, -180.0));
    }

    #[test]
    fn test_parse_fractional_degrees() {
        // Test precise fractional values
        let test_data = OpenTrackData {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 12.3456789,
            pitch: -0.123456,
            roll: 0.000001,
        };

        let bytes = test_data.to_bytes();
        let parsed = OpenTrackData::from_bytes(&bytes);

        assert!(approx_eq(parsed.yaw, 12.3456789));
        assert!(approx_eq(parsed.pitch, -0.123456));
        assert!(approx_eq(parsed.roll, 0.000001));
    }

    #[test]
    fn test_round_trip() {
        // Verify to_bytes/from_bytes round trip
        let original = OpenTrackData {
            x: 123.456,
            y: -789.012,
            z: 345.678,
            yaw: 67.89,
            pitch: -12.34,
            roll: 5.678,
        };

        let bytes = original.to_bytes();
        let parsed = OpenTrackData::from_bytes(&bytes);

        assert_eq!(original, parsed);
    }

    #[test]
    fn test_is_finite_accepts_normal_data() {
        let data = OpenTrackData {
            x: 1.0,
            y: -2.0,
            z: 3.0,
            yaw: 45.0,
            pitch: -30.0,
            roll: 15.0,
        };
        assert!(data.is_finite());
    }

    #[test]
    fn test_is_finite_rejects_nan_and_inf() {
        // Each non-finite field, one at a time, must fail validation so a
        // single bad packet can never reach the smoother.
        let bad_values = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
        for &bad in &bad_values {
            let base = OpenTrackData {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                yaw: 0.0,
                pitch: 0.0,
                roll: 0.0,
            };
            for field in 0..6 {
                let mut d = base;
                match field {
                    0 => d.x = bad,
                    1 => d.y = bad,
                    2 => d.z = bad,
                    3 => d.yaw = bad,
                    4 => d.pitch = bad,
                    _ => d.roll = bad,
                }
                assert!(
                    !d.is_finite(),
                    "field {} = {:?} should be rejected",
                    field,
                    bad
                );
            }
        }
    }

    #[test]
    fn test_is_finite_on_parsed_nan_packet() {
        // A NaN that arrives over the wire must be caught after parsing.
        let mut buf = [0u8; PACKET_SIZE];
        buf[24..32].copy_from_slice(&f64::NAN.to_le_bytes()); // yaw
        let data = OpenTrackData::from_bytes(&buf);
        assert!(!data.is_finite());
    }

    #[test]
    fn test_packet_size_constant() {
        // Verify PACKET_SIZE matches 6 * 8 bytes
        assert_eq!(PACKET_SIZE, 48);
        assert_eq!(PACKET_SIZE, 6 * std::mem::size_of::<f64>());
    }

    #[test]
    fn test_port_constant() {
        assert_eq!(OPENTRACK_PORT, 4242);
    }

    /// Integration test: verify UDP receiver can receive and parse packets
    ///
    /// This test starts the receiver on an alternate port (to avoid conflicts),
    /// sends a test packet, and verifies the receiver correctly processes it.
    #[test]
    fn test_udp_packet_parsing_integration() {
        // Use a different port to avoid conflicts with actual OpenTrack
        let test_port = 14242;

        // Create sender and receiver sockets
        let receiver = UdpSocket::bind(format!("127.0.0.1:{}", test_port)).unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();

        let sender = UdpSocket::bind("127.0.0.1:0").unwrap();

        // Create test data
        let test_data = OpenTrackData {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 42.5,
            pitch: -15.0,
            roll: 7.25,
        };

        // Send packet
        let bytes = test_data.to_bytes();
        sender
            .send_to(&bytes, format!("127.0.0.1:{}", test_port))
            .unwrap();

        // Receive and verify
        let mut buf = [0u8; PACKET_SIZE];
        let (len, _) = receiver.recv_from(&mut buf).unwrap();

        assert_eq!(len, PACKET_SIZE);

        let received = OpenTrackData::from_bytes(&buf);
        assert!(approx_eq(received.yaw, 42.5));
        assert!(approx_eq(received.pitch, -15.0));
        assert!(approx_eq(received.roll, 7.25));
    }

    #[test]
    fn parses_headcam_recenter_trailer() {
        let mut packet = [0u8; TRAILER_SIZE];
        packet[48..52].copy_from_slice(TRAILER_MAGIC);
        packet[52] = TRAILER_VERSION;
        packet[53] = 7;

        assert_eq!(parse_recenter_counter(&packet), Some(7));
        assert_eq!(parse_recenter_counter(&packet[..PACKET_SIZE]), None);
    }

    /// Build a datagram with the given pose and, optionally, an HCAM
    /// trailer carrying `counter`.
    fn datagram(data: &OpenTrackData, trailer: Option<(u8, u8)>) -> Vec<u8> {
        let mut packet = data.to_bytes().to_vec();
        if let Some((version, counter)) = trailer {
            packet.extend_from_slice(TRAILER_MAGIC);
            packet.push(version);
            packet.push(counter);
        }
        packet
    }

    fn level_pose() -> OpenTrackData {
        OpenTrackData {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
        }
    }

    #[test]
    fn trailer_version_is_forward_compatible() {
        let pose = level_pose();
        assert_eq!(
            parse_recenter_counter(&datagram(&pose, Some((2, 3)))),
            Some(3),
            "a newer trailer version must still yield its counter"
        );
        assert_eq!(
            parse_recenter_counter(&datagram(&pose, Some((0, 3)))),
            None,
            "version 0 is not a valid trailer"
        );
    }

    #[test]
    fn first_sighting_presses_and_repeats_do_not() {
        let pose = level_pose();
        let mut detector = RecenterDetector::new();
        let now = Instant::now();

        assert!(
            detector.observe(&datagram(&pose, Some((TRAILER_VERSION, 4))), now),
            "first trailer sighting is a press"
        );
        assert!(
            !detector.observe(
                &datagram(&pose, Some((TRAILER_VERSION, 4))),
                now + Duration::from_millis(16)
            ),
            "the rest of the burst repeats the counter and is not a press"
        );
        assert!(
            detector.observe(
                &datagram(&pose, Some((TRAILER_VERSION, 5))),
                now + Duration::from_millis(32)
            ),
            "a counter change is a press"
        );
        assert!(
            !detector.observe(&datagram(&pose, None), now + Duration::from_millis(48)),
            "a steady-state 48-byte packet is not a press"
        );
    }

    #[test]
    fn counter_wrap_is_a_press() {
        let pose = level_pose();
        let mut detector = RecenterDetector::new();
        let now = Instant::now();

        detector.observe(&datagram(&pose, Some((TRAILER_VERSION, 255))), now);
        assert!(
            detector.observe(
                &datagram(&pose, Some((TRAILER_VERSION, 0))),
                now + Duration::from_millis(16)
            ),
            "the counter wraps, so 255 -> 0 is a change and a press"
        );
    }

    #[test]
    fn wifi_stall_inside_a_burst_does_not_re_press() {
        // A Wi-Fi loss burst mid-recenter-burst must not re-arm
        // first-sighting: the burst's tail carries the SAME counter, and
        // re-arming makes it read as a second press that recentres on
        // whatever pose the head has drifted to since.
        let pose = level_pose();
        let mut detector = RecenterDetector::new();
        let now = Instant::now();

        assert!(detector.observe(&datagram(&pose, Some((TRAILER_VERSION, 9))), now));
        assert!(
            !detector.observe(
                &datagram(&pose, Some((TRAILER_VERSION, 9))),
                now + Duration::from_millis(900)
            ),
            "a 900ms stall is a dropped-packet burst, not a tracker restart"
        );
    }

    #[test]
    fn silence_re_arms_first_sighting() {
        // A tracker app restart resets its counter, so after real silence
        // the next trailer must be treated as a fresh first sighting even
        // when it repeats the counter latched from the old session.
        let pose = level_pose();
        let mut detector = RecenterDetector::new();
        let now = Instant::now();

        assert!(detector.observe(&datagram(&pose, Some((TRAILER_VERSION, 9))), now));
        assert!(
            detector.observe(
                &datagram(&pose, Some((TRAILER_VERSION, 9))),
                now + RECENTER_REARM
            ),
            "packet silence past the re-arm window restores first-sighting"
        );
    }

    #[test]
    fn discarded_packet_discards_its_press() {
        // A press only means "centre on the pose in this packet". If the
        // packet is dropped, honouring the press would centre on the
        // previous, pre-press pose instead.
        let mut nan_pose = level_pose();
        nan_pose.yaw = f64::NAN;
        let mut detector = RecenterDetector::new();

        assert!(
            decode_datagram(
                &mut detector,
                &datagram(&nan_pose, Some((TRAILER_VERSION, 1))),
                Instant::now()
            )
            .is_none(),
            "a non-finite pose is dropped whole, press included"
        );
    }

    #[test]
    fn decoded_press_rides_the_zeroed_pose() {
        let pose = level_pose();
        let mut detector = RecenterDetector::new();
        let (data, pressed) = decode_datagram(
            &mut detector,
            &datagram(&pose, Some((TRAILER_VERSION, 1))),
            Instant::now(),
        )
        .expect("a finite pose with a trailer decodes");

        assert!(pressed);
        assert!(approx_eq(data.yaw, 0.0));
    }
}
