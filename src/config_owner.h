#pragma once

// The mod's settings in CameraUnlock.ini, read and written through cameraunlock-core's
// ConfigOwner. The Rust half reaches this through the C functions in config_owner.cpp, and
// the tests through tests/support/test_support.cpp.

#include <cstddef>
#include <cstdint>
#include <string>

#include "cameraunlock/config/config_owner.h"
#include "cameraunlock/config/config_table.h"
#include "cameraunlock/config/defaults_file.h"
#include "cameraunlock/config/legacy_import.h"
#include "cameraunlock/math/smoothing_utils.h"

extern "C" {

// What the frozen reader in src/legacy_config gave, filled by the Rust side.
struct BsrLegacyConfig {
    // 0: read. 1: no file. 2: a file that could not be read as UTF-8 text, where v0.5.0
    // wrote its template over it and ran on the defaults.
    std::int32_t outcome;
    std::uint8_t world_space_yaw;
    std::int32_t yaw_mode_key;
    double local_smoothing;
    double remote_smoothing;
};

// Runs the frozen reader on the legacy file, a path of `len` UTF-16 units.
typedef void (*BsrLegacyReader)(const wchar_t* path, std::size_t len, BsrLegacyConfig* out);

// A line for the mod's log: level 0 info, 1 warning, 2 an exception out of core.
typedef void (*BsrText)(void* context, std::int32_t level, const char* text, std::size_t len);

// The settings the session starts on. The hotkey lists stay on this side.
struct BsrSettings {
    std::uint16_t udp_port;
    std::uint8_t enable_on_startup;
    std::uint8_t world_space_yaw;
    std::uint8_t rotation_enabled;
    std::uint8_t position_enabled;
    double local_smoothing;
    double remote_smoothing;
};

}  // extern "C"

namespace BioShockRemasteredHeadTracking {

// The settings, as CameraUnlock.ini holds them. The member initialisers are the defaults: the
// table renders them into the file the mod creates at first launch.
struct Config {
    std::uint16_t udp_port = 4242;
    bool enable_on_startup = true;
    // true = horizon-locked (world-space) yaw, false = camera-local yaw.
    bool world_space_yaw = true;
    // The tracking mode the session starts in, as a pair; both true is rotation and position.
    bool rotation_enabled = true;
    bool position_enabled = true;
    // double, because the pipeline smooths in double and v0.5.0 read these as double.
    double local_smoothing = cameraunlock::math::kDefaultLocalSmoothing;
    double remote_smoothing = cameraunlock::math::kDefaultRemoteSmoothing;
    std::string toggle_key{cameraunlock::config::schema::ConceptTraits<
        cameraunlock::config::schema::Concept::ToggleKey>::kCanonicalDefault};
    std::string cycle_tracking_mode_key{cameraunlock::config::schema::ConceptTraits<
        cameraunlock::config::schema::Concept::CycleTrackingModeKey>::kCanonicalDefault};
    std::string yaw_mode_key{cameraunlock::config::schema::ConceptTraits<
        cameraunlock::config::schema::Concept::YawModeKey>::kCanonicalDefault};
};

// The rows of CameraUnlock.ini. Only the tracking mode pair and WorldSpaceYaw are Writable: the
// mode and yaw hotkeys save the player's choice, and End changes the session only.
cameraunlock::config::ConfigTable<Config> ConfigTable();

cameraunlock::config::RenderHeader ConfigHeader();

// bioshock_headtrack.ini as v0.5.0 read it, through the frozen reader, mapped into Config.
cameraunlock::config::LegacyImport<Config> ConfigLegacyImport(BsrLegacyReader reader);

// The owner's options for the files in `folder` (with its trailing separator): the settings in
// CameraUnlock.ini, imported once from bioshock_headtrack.ini, which is never written. The mod
// passes DefaultsFile::PerUser() and a test a scratch file.
cameraunlock::config::ConfigOwnerOptions<Config> ConfigOptions(const std::wstring& folder,
                                                               cameraunlock::config::DefaultsFile defaults,
                                                               BsrLegacyReader reader);

}  // namespace BioShockRemasteredHeadTracking

// One owner and the settings its last Load gave, behind the C functions.
struct BsrConfigOwner {
    explicit BsrConfigOwner(cameraunlock::config::ConfigOwnerOptions<BioShockRemasteredHeadTracking::Config> options)
        : owner(std::move(options)) {}

    cameraunlock::config::ConfigOwner<BioShockRemasteredHeadTracking::Config> owner;
    BioShockRemasteredHeadTracking::Config loaded;
};
