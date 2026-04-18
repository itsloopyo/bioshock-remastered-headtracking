#![allow(dead_code)]
//! Global tracking state management
//!
//! Provides thread-safe global state shared between UDP receiver,
//! hotkey handler, and DirectX render hook threads.
//!
//! # State Sharing
//!
//! The `GLOBAL_STATE` is a lazy-initialized, thread-safe singleton that provides:
//!
//! - **UDP Receiver Thread**: Updates yaw/pitch/roll values at ~250Hz
//! - **Hotkey Handler Thread**: Modifies enabled flag and recenter offset
//! - **DirectX Render Hook**: Reads state each frame to apply camera rotation
//!
//! # Thread Safety
//!
//! Uses a hybrid approach for optimal performance:
//! - **Atomics** for frequently accessed rotation values (lock-free)
//! - **RwLock** for less frequently accessed state (toggle, recenter)
//!
//! This eliminates lock contention on the hot path (rotation reads at 60-120Hz)
//! while maintaining proper synchronization for control operations.
//!
//! # Performance
//!
//! Rotation values use `AtomicU64` storing f64 bits for lock-free access.
//! This provides ~10x faster reads compared to RwLock for the hot path.
//!
//! # Auto-enable
//!
//! `enabled` defaults to `true` so head tracking is active immediately when
//! the game starts - users expect to plug in OpenTrack and have it just work.

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Atomic rotation storage for lock-free access on the hot path
///
/// Stores f64 rotation values as AtomicU64 bits for lock-free reads/writes.
/// This eliminates RwLock contention on the render hook hot path.
pub struct AtomicRotation {
    yaw: AtomicU64,
    pitch: AtomicU64,
    roll: AtomicU64,
}

impl AtomicRotation {
    /// Create new atomic rotation initialized to zero
    pub const fn new() -> Self {
        Self {
            yaw: AtomicU64::new(0),
            pitch: AtomicU64::new(0),
            roll: AtomicU64::new(0),
        }
    }

    /// Store rotation values (called by UDP receiver at ~250Hz)
    #[inline(always)]
    pub fn store(&self, yaw: f64, pitch: f64, roll: f64) {
        self.yaw.store(yaw.to_bits(), Ordering::Release);
        self.pitch.store(pitch.to_bits(), Ordering::Release);
        self.roll.store(roll.to_bits(), Ordering::Release);
    }

    /// Load rotation values (called by render hook at frame rate)
    #[inline(always)]
    pub fn load(&self) -> (f64, f64, f64) {
        (
            f64::from_bits(self.yaw.load(Ordering::Acquire)),
            f64::from_bits(self.pitch.load(Ordering::Acquire)),
            f64::from_bits(self.roll.load(Ordering::Acquire)),
        )
    }

    /// Get current yaw value
    #[inline(always)]
    pub fn yaw(&self) -> f64 {
        f64::from_bits(self.yaw.load(Ordering::Acquire))
    }

    /// Get current pitch value
    #[inline(always)]
    pub fn pitch(&self) -> f64 {
        f64::from_bits(self.pitch.load(Ordering::Acquire))
    }

    /// Get current roll value
    #[inline(always)]
    pub fn roll(&self) -> f64 {
        f64::from_bits(self.roll.load(Ordering::Acquire))
    }
}

impl std::fmt::Debug for AtomicRotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (yaw, pitch, roll) = self.load();
        f.debug_struct("AtomicRotation")
            .field("yaw", &yaw)
            .field("pitch", &pitch)
            .field("roll", &roll)
            .finish()
    }
}

impl Default for AtomicRotation {
    fn default() -> Self {
        Self::new()
    }
}

/// Global tracking state shared across all threads
#[derive(Debug)]
pub struct TrackingState {
    /// Master enable flag (End / Ctrl+Shift+Y).
    pub enabled: bool,

    /// Current yaw rotation from OpenTrack (degrees) - LEGACY, use atomic_rotation
    pub yaw: f64,

    /// Current pitch rotation from OpenTrack (degrees) - LEGACY, use atomic_rotation
    pub pitch: f64,

    /// Current roll rotation from OpenTrack (degrees) - LEGACY, use atomic_rotation
    pub roll: f64,

    /// Recenter offset, subtracted from rotation values (Home / Ctrl+Shift+T).
    pub recenter_offset: (f64, f64, f64),

    /// 6DOF positional tracking toggle.
    pub position_enabled: bool,

    /// True when in active gameplay, false during menus/cutscenes
    pub gameplay_active: bool,

    /// Debounce timer for the tracking-enable toggle.
    pub last_toggle_time: Instant,

    /// Debounce timer for the recenter hotkey.
    pub last_recenter_time: Instant,

    /// Debounce timer for the position toggle.
    pub last_position_time: Instant,

    /// Signal for threads to shutdown
    pub shutdown_requested: bool,
}

/// Lock-free atomic rotation values for hot path access
///
/// Use this for reading rotation values in the render hook to avoid
/// RwLock contention. Updated by UDP receiver, read by render hook.
pub static ATOMIC_ROTATION: AtomicRotation = AtomicRotation::new();

/// Atomic recenter offset for lock-free access
pub static ATOMIC_RECENTER: AtomicRotation = AtomicRotation::new();

/// Lock-free atomic position (x, y, z) values from the OpenTrack
/// packet, in centimetres. Reuses the `AtomicRotation` slot type
/// (three lock-free f64 fields) for consistency with rotation -
/// the field names are misnomers in this case but the storage is
/// the same.
pub static ATOMIC_POSITION: AtomicRotation = AtomicRotation::new();

/// Atomic position-recenter offset (cm). Subtracted from raw
/// position to give the head's displacement from the recenter origin.
pub static ATOMIC_POSITION_RECENTER: AtomicRotation = AtomicRotation::new();

/// Atomic enabled flag for lock-free access
pub static ATOMIC_ENABLED: AtomicBool = AtomicBool::new(true);

/// Atomic gameplay_active flag for lock-free access
/// Starts true so tracking works immediately (state detector also defaults to Gameplay)
pub static ATOMIC_GAMEPLAY_ACTIVE: AtomicBool = AtomicBool::new(true);

impl Default for TrackingState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            enabled: true,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            recenter_offset: (0.0, 0.0, 0.0),
            position_enabled: true,
            // Start active - state detector defaults to Gameplay
            gameplay_active: true,
            last_toggle_time: now,
            last_recenter_time: now,
            last_position_time: now,
            shutdown_requested: false,
        }
    }
}

impl TrackingState {
    /// Get the recentered rotation values
    pub fn get_recentered_rotation(&self) -> (f64, f64, f64) {
        (
            self.yaw - self.recenter_offset.0,
            self.pitch - self.recenter_offset.1,
            self.roll - self.recenter_offset.2,
        )
    }

    /// Capture current rotation AND position as the recenter origin.
    /// After this, future head-tracking deltas are measured relative
    /// to the head pose at the moment of the press.
    pub fn set_recenter(&mut self) {
        self.recenter_offset = (self.yaw, self.pitch, self.roll);
        ATOMIC_RECENTER.store(self.yaw, self.pitch, self.roll);
        let (px, py, pz) = ATOMIC_POSITION.load();
        ATOMIC_POSITION_RECENTER.store(px, py, pz);
        log::info!(
            "Recentered: yaw={:.2}° pitch={:.2}° roll={:.2}°  pos=({:.2},{:.2},{:.2})cm",
            self.yaw,
            self.pitch,
            self.roll,
            px,
            py,
            pz
        );
    }

    /// Toggle enabled state
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        // Sync to atomic enabled flag
        ATOMIC_ENABLED.store(self.enabled, Ordering::Release);
        log::info!(
            "Head tracking {}",
            if self.enabled { "enabled" } else { "disabled" }
        );
    }

    /// Toggle 6DOF position tracking.
    pub fn toggle_position(&mut self) {
        self.position_enabled = !self.position_enabled;
        ATOMIC_POSITION_ENABLED.store(self.position_enabled, Ordering::Release);
        log::info!(
            "Position tracking {}",
            if self.position_enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
}

/// Get recentered rotation values using lock-free atomics
///
/// This is the optimized hot path for reading rotation values in the render hook.
/// Uses atomic operations instead of RwLock for ~10x faster access.
///
/// # Performance
///
/// This function avoids any lock acquisition and uses memory ordering
/// to ensure proper synchronization between the UDP receiver (writer)
/// and render hook (reader).
#[inline(always)]
pub fn get_recentered_rotation_atomic() -> (f64, f64, f64) {
    let (yaw, pitch, roll) = ATOMIC_ROTATION.load();
    let (offset_yaw, offset_pitch, offset_roll) = ATOMIC_RECENTER.load();
    (yaw - offset_yaw, pitch - offset_pitch, roll - offset_roll)
}

/// Check if head tracking is enabled using lock-free atomic
#[inline(always)]
pub fn is_enabled_atomic() -> bool {
    ATOMIC_ENABLED.load(Ordering::Acquire)
}

/// Check if gameplay is active using lock-free atomic
#[inline(always)]
pub fn is_gameplay_active_atomic() -> bool {
    ATOMIC_GAMEPLAY_ACTIVE.load(Ordering::Acquire)
}

/// Set gameplay active state atomically
#[inline(always)]
pub fn set_gameplay_active_atomic(active: bool) {
    ATOMIC_GAMEPLAY_ACTIVE.store(active, Ordering::Release);
}

/// Update rotation values atomically (called by UDP receiver)
#[inline(always)]
pub fn update_rotation_atomic(yaw: f64, pitch: f64, roll: f64) {
    ATOMIC_ROTATION.store(yaw, pitch, roll);
}

/// Update raw position values atomically (called by UDP receiver).
/// Inputs are OpenTrack-frame centimetres: x = right, y = up,
/// z = away-from-screen.
#[inline(always)]
pub fn update_position_atomic(x: f64, y: f64, z: f64) {
    ATOMIC_POSITION.store(x, y, z);
}

/// Get recentered position deltas in head-frame centimetres:
/// `(right, up, forward)`. Sign conventions, all 1:1 with no
/// sensitivity scaling:
///   - `right`   = `-(x - ox)` - OpenTrack X is inverted relative to
///     what BSR's camera basis expects, so the lateral axis gets a
///     leading minus.
///   - `up`      = `y - oy` - passes through.
///   - `forward` = `-(z - oz)` - OpenTrack `+Z = back`, we want
///     `+forward = lean toward screen`, so negate.
#[inline(always)]
pub fn get_recentered_position_atomic() -> (f64, f64, f64) {
    let (x, y, z) = ATOMIC_POSITION.load();
    let (ox, oy, oz) = ATOMIC_POSITION_RECENTER.load();
    (-(x - ox), y - oy, -(z - oz))
}

/// Lock-free check for the position-tracking toggle. The hotkey
/// thread mutates `position_enabled` under the global write-lock;
/// this static mirror is updated alongside it for hot-path reads.
pub static ATOMIC_POSITION_ENABLED: AtomicBool = AtomicBool::new(true);

#[inline(always)]
pub fn is_position_enabled_atomic() -> bool {
    ATOMIC_POSITION_ENABLED.load(Ordering::Acquire)
}

/// The head offset that engine_hook actually applied this frame, in
/// body-frame centimetres `(right, up, forward)` - i.e. what
/// `get_recentered_position_atomic()` returned, clamped to the
/// per-axis limits, AND zeroed when position tracking is toggled
/// off. The overlay reads this so the reticle can compensate for
/// parallax: with positional tracking on, the rendered view shifts
/// relative to where the gun is aimed, so the reticle has to shift
/// too to stay glued to the bullet hit point.
pub static ATOMIC_APPLIED_HEAD_OFFSET: AtomicRotation = AtomicRotation::new();

#[inline(always)]
pub fn store_applied_head_offset(right: f64, up: f64, forward: f64) {
    ATOMIC_APPLIED_HEAD_OFFSET.store(right, up, forward);
}

#[inline(always)]
pub fn applied_head_offset() -> (f64, f64, f64) {
    ATOMIC_APPLIED_HEAD_OFFSET.load()
}

/// Lazy-initialized global state, wrapped in Arc<RwLock<>> for thread safety
///
/// # Thread Safety
///
/// - Use `.read()` to acquire a read lock for reading state
/// - Use `.write()` to acquire a write lock for modifying state
/// - Read locks can be held simultaneously by multiple threads
/// - Write locks are exclusive
///
/// # Usage
///
/// ```rust,ignore
/// // Reading state (in render hook)
/// let (yaw, pitch, roll) = {
///     let state = GLOBAL_STATE.read();
///     state.get_recentered_rotation()
/// };
///
/// // Modifying state (in hotkey handler)
/// {
///     let mut state = GLOBAL_STATE.write();
///     state.toggle();
/// }
/// ```
pub static GLOBAL_STATE: Lazy<Arc<RwLock<TrackingState>>> =
    Lazy::new(|| Arc::new(RwLock::new(TrackingState::default())));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_enabled_true() {
        let state = TrackingState::default();
        assert!(state.enabled, "Default state should have enabled=true");
    }

    #[test]
    fn test_default_rotations_zero() {
        let state = TrackingState::default();
        assert!(
            (state.yaw - 0.0).abs() < f64::EPSILON,
            "Default yaw should be 0"
        );
        assert!(
            (state.pitch - 0.0).abs() < f64::EPSILON,
            "Default pitch should be 0"
        );
        assert!(
            (state.roll - 0.0).abs() < f64::EPSILON,
            "Default roll should be 0"
        );
    }

    #[test]
    fn test_default_recenter_offset_zero() {
        let state = TrackingState::default();
        assert!(
            (state.recenter_offset.0 - 0.0).abs() < f64::EPSILON,
            "Default recenter yaw should be 0"
        );
        assert!(
            (state.recenter_offset.1 - 0.0).abs() < f64::EPSILON,
            "Default recenter pitch should be 0"
        );
        assert!(
            (state.recenter_offset.2 - 0.0).abs() < f64::EPSILON,
            "Default recenter roll should be 0"
        );
    }

    #[test]
    fn test_default_gameplay_active() {
        let state = TrackingState::default();
        assert!(
            state.gameplay_active,
            "Default gameplay_active should be true for immediate tracking"
        );
    }

    #[test]
    fn test_default_shutdown_not_requested() {
        let state = TrackingState::default();
        assert!(
            !state.shutdown_requested,
            "Default shutdown_requested should be false"
        );
    }

    #[test]
    fn test_toggle_changes_state() {
        let mut state = TrackingState::default();
        assert!(state.enabled);

        state.toggle();
        assert!(!state.enabled, "Toggle should disable when enabled");

        state.toggle();
        assert!(state.enabled, "Toggle should enable when disabled");
    }

    #[test]
    fn test_toggle_logs_message() {
        // Verify toggle method logs appropriately
        // This is a structural test - the actual logging is verified by log output
        let mut state = TrackingState::default();
        state.toggle(); // Should log "Head tracking disabled"
        state.toggle(); // Should log "Head tracking enabled"
                        // No assertions needed - just verifying no panics
    }

    #[test]
    fn test_set_recenter_captures_current() {
        let mut state = TrackingState::default();
        state.yaw = 45.5;
        state.pitch = -12.3;
        state.roll = 8.7;

        state.set_recenter();

        assert!((state.recenter_offset.0 - 45.5).abs() < 0.0001);
        assert!((state.recenter_offset.1 - (-12.3)).abs() < 0.0001);
        assert!((state.recenter_offset.2 - 8.7).abs() < 0.0001);
    }

    #[test]
    fn test_get_recentered_rotation_subtracts_offset() {
        let mut state = TrackingState::default();
        state.yaw = 90.0;
        state.pitch = 30.0;
        state.roll = -15.0;
        state.recenter_offset = (45.0, 10.0, -5.0);

        let (y, p, r) = state.get_recentered_rotation();

        assert!(
            (y - 45.0).abs() < 0.0001,
            "Recentered yaw should be 90 - 45 = 45"
        );
        assert!(
            (p - 20.0).abs() < 0.0001,
            "Recentered pitch should be 30 - 10 = 20"
        );
        assert!(
            (r - (-10.0)).abs() < 0.0001,
            "Recentered roll should be -15 - (-5) = -10"
        );
    }

    #[test]
    fn test_get_recentered_rotation_zero_offset() {
        let mut state = TrackingState::default();
        state.yaw = 45.0;
        state.pitch = 20.0;
        state.roll = -10.0;
        // recenter_offset is (0, 0, 0) by default

        let (y, p, r) = state.get_recentered_rotation();

        assert!(
            (y - 45.0).abs() < 0.0001,
            "With zero offset, yaw should be unchanged"
        );
        assert!(
            (p - 20.0).abs() < 0.0001,
            "With zero offset, pitch should be unchanged"
        );
        assert!(
            (r - (-10.0)).abs() < 0.0001,
            "With zero offset, roll should be unchanged"
        );
    }

    #[test]
    fn test_recenter_then_get_rotation_is_zero() {
        let mut state = TrackingState::default();
        state.yaw = 60.0;
        state.pitch = -25.0;
        state.roll = 15.0;

        // Recenter at current position
        state.set_recenter();

        // After recentering, effective rotation should be zero
        let (y, p, r) = state.get_recentered_rotation();

        assert!(
            (y - 0.0).abs() < 0.0001,
            "After recenter, effective yaw should be 0"
        );
        assert!(
            (p - 0.0).abs() < 0.0001,
            "After recenter, effective pitch should be 0"
        );
        assert!(
            (r - 0.0).abs() < 0.0001,
            "After recenter, effective roll should be 0"
        );
    }

    #[test]
    fn test_rotation_after_recenter() {
        let mut state = TrackingState::default();

        // Initial position
        state.yaw = 30.0;
        state.pitch = 10.0;
        state.roll = -5.0;

        // Recenter here
        state.set_recenter();

        // Move head 15 degrees right (yaw)
        state.yaw = 45.0;
        state.pitch = 10.0;
        state.roll = -5.0;

        let (y, p, r) = state.get_recentered_rotation();

        assert!(
            (y - 15.0).abs() < 0.0001,
            "Should show 15 degree yaw from center"
        );
        assert!((p - 0.0).abs() < 0.0001, "Pitch unchanged from center");
        assert!((r - 0.0).abs() < 0.0001, "Roll unchanged from center");
    }

    #[test]
    fn test_extreme_rotation_values() {
        let mut state = TrackingState::default();
        state.yaw = 180.0;
        state.pitch = 90.0;
        state.roll = -90.0;
        state.recenter_offset = (-180.0, -90.0, 90.0);

        let (y, p, r) = state.get_recentered_rotation();

        assert!((y - 360.0).abs() < 0.0001);
        assert!((p - 180.0).abs() < 0.0001);
        assert!((r - (-180.0)).abs() < 0.0001);
    }

    #[test]
    fn test_global_state_thread_safety() {
        // Verify we can read and write from the global state
        {
            let mut state = GLOBAL_STATE.write();
            state.yaw = 42.0;
        }

        {
            let state = GLOBAL_STATE.read();
            assert!((state.yaw - 42.0).abs() < 0.0001);
        }

        // Reset for other tests
        {
            let mut state = GLOBAL_STATE.write();
            state.yaw = 0.0;
        }
    }

    #[test]
    fn test_global_state_multiple_reads() {
        // RwLock should allow multiple simultaneous readers
        let state1 = GLOBAL_STATE.read();
        let state2 = GLOBAL_STATE.read();

        // Both should see the same value
        assert_eq!(state1.enabled, state2.enabled);

        // Drop locks explicitly
        drop(state1);
        drop(state2);
    }

    // =========================================================================
    // Atomic Rotation Tests (Optimized Hot Path)
    // =========================================================================

    #[test]
    fn test_atomic_rotation_store_load() {
        let rotation = AtomicRotation::new();

        // Store values
        rotation.store(45.0, 30.0, 15.0);

        // Load and verify
        let (yaw, pitch, roll) = rotation.load();
        assert!((yaw - 45.0).abs() < f64::EPSILON);
        assert!((pitch - 30.0).abs() < f64::EPSILON);
        assert!((roll - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_atomic_rotation_individual_accessors() {
        let rotation = AtomicRotation::new();
        rotation.store(10.0, 20.0, 30.0);

        assert!((rotation.yaw() - 10.0).abs() < f64::EPSILON);
        assert!((rotation.pitch() - 20.0).abs() < f64::EPSILON);
        assert!((rotation.roll() - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_atomic_rotation_negative_values() {
        let rotation = AtomicRotation::new();
        rotation.store(-45.0, -30.0, -15.0);

        let (yaw, pitch, roll) = rotation.load();
        assert!((yaw - (-45.0)).abs() < f64::EPSILON);
        assert!((pitch - (-30.0)).abs() < f64::EPSILON);
        assert!((roll - (-15.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_global_atomic_rotation() {
        // Test the global ATOMIC_ROTATION static
        update_rotation_atomic(90.0, 45.0, 22.5);

        let (yaw, pitch, roll) = ATOMIC_ROTATION.load();
        assert!((yaw - 90.0).abs() < f64::EPSILON);
        assert!((pitch - 45.0).abs() < f64::EPSILON);
        assert!((roll - 22.5).abs() < f64::EPSILON);

        // Reset for other tests
        update_rotation_atomic(0.0, 0.0, 0.0);
    }

    #[test]
    fn test_get_recentered_rotation_atomic() {
        // Set rotation and recenter offset
        ATOMIC_ROTATION.store(90.0, 45.0, 30.0);
        ATOMIC_RECENTER.store(30.0, 15.0, 10.0);

        let (y, p, r) = get_recentered_rotation_atomic();

        assert!(
            (y - 60.0).abs() < 0.0001,
            "Recentered yaw should be 90 - 30 = 60"
        );
        assert!(
            (p - 30.0).abs() < 0.0001,
            "Recentered pitch should be 45 - 15 = 30"
        );
        assert!(
            (r - 20.0).abs() < 0.0001,
            "Recentered roll should be 30 - 10 = 20"
        );

        // Reset for other tests
        ATOMIC_ROTATION.store(0.0, 0.0, 0.0);
        ATOMIC_RECENTER.store(0.0, 0.0, 0.0);
    }

    #[test]
    fn test_atomic_enabled_flag() {
        // Default should be true (auto-enable).
        assert!(is_enabled_atomic());

        // Toggle off
        ATOMIC_ENABLED.store(false, Ordering::Release);
        assert!(!is_enabled_atomic());

        // Toggle on
        ATOMIC_ENABLED.store(true, Ordering::Release);
        assert!(is_enabled_atomic());
    }

    #[test]
    fn test_atomic_gameplay_active_flag() {
        // Set active
        set_gameplay_active_atomic(true);
        assert!(is_gameplay_active_atomic());

        // Set inactive
        set_gameplay_active_atomic(false);
        assert!(!is_gameplay_active_atomic());
    }

    #[test]
    fn test_atomic_rotation_thread_safety() {
        use std::thread;

        // Spawn multiple writers
        let handles: Vec<_> = (0..4)
            .map(|i| {
                thread::spawn(move || {
                    for j in 0..100 {
                        let val = (i * 100 + j) as f64;
                        ATOMIC_ROTATION.store(val, val, val);
                    }
                })
            })
            .collect();

        // Wait for all writers
        for h in handles {
            h.join().unwrap();
        }

        // Verify we can still read (no corruption)
        let (yaw, pitch, roll) = ATOMIC_ROTATION.load();
        assert!(yaw.is_finite());
        assert!(pitch.is_finite());
        assert!(roll.is_finite());

        // Reset
        ATOMIC_ROTATION.store(0.0, 0.0, 0.0);
    }
}
