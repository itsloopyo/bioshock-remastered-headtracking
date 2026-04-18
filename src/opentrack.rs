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
use std::net::UdpSocket;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::tracking::{update_position_atomic, update_rotation_atomic, GLOBAL_STATE};

/// OpenTrack UDP port (project-standard default).
pub const OPENTRACK_PORT: u16 = 4242;

/// OpenTrack packet size: 6 doubles * 8 bytes = 48 bytes
pub const PACKET_SIZE: usize = 48;

/// Socket read timeout in milliseconds (4ms allows ~250Hz polling)
const READ_TIMEOUT_MS: u64 = 4;

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

/// Start the OpenTrack UDP receiver thread
///
/// Binds to 0.0.0.0:4242 and continuously receives packets,
/// updating the global tracking state with rotation values. Binding to all
/// interfaces lets phone-based trackers send directly without an OpenTrack
/// relay on the PC.
pub fn start_receiver() -> io::Result<JoinHandle<()>> {
    let socket = UdpSocket::bind(format!("0.0.0.0:{}", OPENTRACK_PORT))?;
    socket.set_read_timeout(Some(Duration::from_millis(READ_TIMEOUT_MS)))?;

    log::info!("OpenTrack receiver started on port {}", OPENTRACK_PORT);

    let handle = thread::spawn(move || {
        let mut buf = [0u8; PACKET_SIZE];

        loop {
            // Check if shutdown requested
            {
                let state = GLOBAL_STATE.read();
                if state.shutdown_requested {
                    log::info!("OpenTrack receiver shutting down");
                    break;
                }
            }

            match socket.recv(&mut buf) {
                Ok(PACKET_SIZE) => {
                    let data = OpenTrackData::from_bytes(&buf);

                    // Update rotation + position using lock-free atomics
                    // (optimized hot path).
                    update_rotation_atomic(data.yaw, data.pitch, data.roll);
                    update_position_atomic(data.x, data.y, data.z);

                    // Also update GLOBAL_STATE for legacy compatibility
                    // This is less frequent than reads, so RwLock overhead is acceptable
                    let mut state = GLOBAL_STATE.write();
                    state.yaw = data.yaw;
                    state.pitch = data.pitch;
                    state.roll = data.roll;
                }
                Ok(size) => {
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
    });

    Ok(handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::time::Duration;

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
}
