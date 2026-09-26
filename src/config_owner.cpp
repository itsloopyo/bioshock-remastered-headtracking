#include "config_owner.h"

#include <exception>
#include <functional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "cameraunlock/input/hotkey_poller.h"
#include "cameraunlock/input/key_binding_registration.h"
#include "cameraunlock/input/key_bindings.h"

namespace BioShockRemasteredHeadTracking {

namespace {

using cameraunlock::config::ImportResult;
using cameraunlock::config::LegacyInput;
using cameraunlock::input::KeyBinding;
using cameraunlock::input::KeyModifiers;

constexpr std::int32_t kLegacyRead = 0;
constexpr std::int32_t kLegacyNoFile = 1;
constexpr std::int32_t kLegacyUnread = 2;

// Everything v0.5.0 did not read from the file stays at its default: it always started with
// tracking on and both axes on, on port 4242, with End or Ctrl+Shift+Y and PageUp or
// Ctrl+Shift+G, which are the defaults here too. Its yaw key always fired on Ctrl+Shift+H as
// well, and the frozen reader only hands back a code from 0x01 to 0xFE.
void MapLegacy(const BsrLegacyConfig& c, Config& out) {
    out.world_space_yaw = c.world_space_yaw != 0;
    out.yaw_mode_key = cameraunlock::input::FormatKeyBindings(
        {KeyBinding{KeyModifiers::kNone, c.yaw_mode_key}, KeyBinding{KeyModifiers::kCtrl | KeyModifiers::kShift, 'H'}});
    out.local_smoothing = c.local_smoothing;
    out.remote_smoothing = c.remote_smoothing;
}

}  // namespace

cameraunlock::config::ConfigTable<Config> ConfigTable() {
    using C = cameraunlock::config::schema::Concept;
    cameraunlock::config::ConfigTable<Config> table{Config{}};
    table.Concept<C::UdpPort>(&Config::udp_port)
        .Concept<C::EnableOnStartup>(&Config::enable_on_startup)
        .Concept<C::WorldSpaceYaw>(&Config::world_space_yaw)
        .Writable()
        .Concept<C::RotationEnabled>(&Config::rotation_enabled)
        .Writable()
        .Concept<C::LocalSmoothing>(&Config::local_smoothing)
        .Concept<C::RemoteSmoothing>(&Config::remote_smoothing)
        .Concept<C::PositionEnabled>(&Config::position_enabled)
        .Writable()
        .Concept<C::ToggleKey>(&Config::toggle_key)
        .Concept<C::CycleTrackingModeKey>(&Config::cycle_tracking_mode_key)
        .Concept<C::YawModeKey>(&Config::yaw_mode_key);
    return table;
}

cameraunlock::config::RenderHeader ConfigHeader() {
    cameraunlock::config::RenderHeader header;
    header.display_name = "BioShock Remastered";
    return header;
}

cameraunlock::config::LegacyImport<Config> ConfigLegacyImport(BsrLegacyReader reader) {
    cameraunlock::config::LegacyImport<Config> import;
    import.run = [reader](const LegacyInput& input, Config& out) {
        BsrLegacyConfig read{};
        reader(input.path.data(), input.path.size(), &read);
        MapLegacy(read, out);
        switch (read.outcome) {
            case kLegacyRead:
            case kLegacyUnread:
                return ImportResult::Imported({});
            case kLegacyNoFile:
                return ImportResult::Absent({});
        }
        throw std::logic_error("the frozen reader gave outcome " + std::to_string(read.outcome));
    };
    // src/legacy_config KEYS. [overlay] fov_h is not here: v0.5.0 only logged it, so the
    // migration logs it as not carried.
    import.keys = {
        {"General", "WorldSpaceYaw"},
        {"Hotkeys", "YawModeKey"},
        {"Smoothing", "LocalSmoothing"},
        {"Smoothing", "RemoteSmoothing"},
    };
    return import;
}

cameraunlock::config::ConfigOwnerOptions<Config> ConfigOptions(const std::wstring& folder,
                                                               cameraunlock::config::DefaultsFile defaults,
                                                               BsrLegacyReader reader) {
    cameraunlock::config::ConfigOwnerOptions<Config> options;
    options.path = folder + L"CameraUnlock.ini";
    options.legacy_path = folder + L"bioshock_headtrack.ini";
    options.table = ConfigTable();
    options.import = ConfigLegacyImport(reader);
    options.header = ConfigHeader();
    options.defaults = std::move(defaults);
    return options;
}

}  // namespace BioShockRemasteredHeadTracking

namespace {

using BioShockRemasteredHeadTracking::Config;

void Say(BsrText text, void* context, std::int32_t level, const std::string& line) {
    text(context, level, line.data(), line.size());
}

// Nothing core throws is a condition the mod can recover from: each is a broken contract.
// The exception's text goes back to the Rust side, which stops the mod with it, since
// unwinding across the C boundary is not defined.
template <class F>
std::int32_t Guarded(BsrText text, void* context, F&& body) {
    try {
        return body();
    } catch (const std::exception& e) {
        Say(text, context, 2, e.what());
        return -1;
    }
}

BsrSettings ToSettings(const Config& c) {
    BsrSettings s{};
    s.udp_port = c.udp_port;
    s.enable_on_startup = c.enable_on_startup;
    s.world_space_yaw = c.world_space_yaw;
    s.rotation_enabled = c.rotation_enabled;
    s.position_enabled = c.position_enabled;
    s.local_smoothing = c.local_smoothing;
    s.remote_smoothing = c.remote_smoothing;
    return s;
}

std::int32_t Save(BsrConfigOwner* owner, const std::function<void(Config&)>& change, BsrText text,
                  void* context) {
    const cameraunlock::config::ConfigSaveResult result = owner->owner.Save(change);
    for (const std::string& line : result.log) Say(text, context, 0, line);
    if (result.status != cameraunlock::config::ConfigSaveStatus::Saved) Say(text, context, 1, result.reason);
    return static_cast<std::int32_t>(result.status);
}

void LogBindings(BsrText text, void* context, const char* name, const std::string& list) {
    Say(text, context, 0, std::string("hotkeys: ") + name + "=" + list);
}

}  // namespace

extern "C" {

typedef void (*BsrAction)(void);

// The mod's owner, reading Defaults.ini where the player keeps it. Null after an exception.
BsrConfigOwner* bsr_config_owner_new(const wchar_t* folder, std::size_t len, BsrLegacyReader reader, BsrText text,
                                     void* context) {
    BsrConfigOwner* owner = nullptr;
    Guarded(text, context, [&]() -> std::int32_t {
        owner = new BsrConfigOwner(BioShockRemasteredHeadTracking::ConfigOptions(
            std::wstring(folder, len), cameraunlock::config::DefaultsFile::PerUser(), reader));
        return 0;
    });
    return owner;
}

void bsr_config_owner_free(BsrConfigOwner* owner) { delete owner; }

// Loads, fills `out` with the settings the session starts on and returns the load status, or
// -1 after an exception.
std::int32_t bsr_config_load(BsrConfigOwner* owner, BsrSettings* out, BsrText text, void* context) {
    return Guarded(text, context, [&]() -> std::int32_t {
        const cameraunlock::config::ConfigLoadResult<Config> result = owner->owner.Load();
        for (const std::string& line : result.log) Say(text, context, 0, line);
        if (!result.reason.empty()) Say(text, context, 1, result.reason);
        Say(text, context, 0,
            std::string("config: ") + cameraunlock::config::ConfigLoadStatusName(result.status));
        owner->loaded = result.config;
        *out = ToSettings(result.config);
        return static_cast<std::int32_t>(result.status);
    });
}

// Saves the tracking mode pair and returns the save status, or -1 after an exception.
std::int32_t bsr_config_save_tracking_mode(BsrConfigOwner* owner, std::uint8_t rotation_enabled,
                                           std::uint8_t position_enabled, BsrText text, void* context) {
    return Guarded(text, context, [&]() -> std::int32_t {
        return Save(
            owner,
            [&](Config& c) {
                c.rotation_enabled = rotation_enabled != 0;
                c.position_enabled = position_enabled != 0;
            },
            text, context);
    });
}

// Saves WorldSpaceYaw and returns the save status, or -1 after an exception.
std::int32_t bsr_config_save_world_space_yaw(BsrConfigOwner* owner, std::uint8_t world_space_yaw, BsrText text,
                                             void* context) {
    return Guarded(text, context, [&]() -> std::int32_t {
        return Save(
            owner, [&](Config& c) { c.world_space_yaw = world_space_yaw != 0; }, text, context);
    });
}

// Registers the three hotkey lists the last Load gave on a poller of their own and starts it.
// Returns 0, or -1 after an exception. The poller is never destroyed: its destructor joins
// its thread, which must not happen under the loader lock at process exit.
std::int32_t bsr_hotkeys_start(BsrConfigOwner* owner, BsrAction toggle, BsrAction cycle_mode, BsrAction yaw_mode,
                               BsrText text, void* context) {
    return Guarded(text, context, [&]() -> std::int32_t {
        static cameraunlock::input::HotkeyPoller* poller = nullptr;
        if (poller != nullptr) throw std::logic_error("the hotkey poller is already running");
        poller = new cameraunlock::input::HotkeyPoller();
        const Config& c = owner->loaded;
        const std::pair<const char*, const std::string*> lists[] = {
            {"ToggleKey", &c.toggle_key},
            {"CycleTrackingModeKey", &c.cycle_tracking_mode_key},
            {"YawModeKey", &c.yaw_mode_key},
        };
        const BsrAction actions[] = {toggle, cycle_mode, yaw_mode};
        for (std::size_t i = 0; i < 3; ++i) {
            // The table's hotkey codec read the list, so it parses.
            const cameraunlock::input::KeyBindingsParseResult parsed =
                cameraunlock::input::ParseKeyBindings(*lists[i].second);
            if (!parsed.ok()) throw std::logic_error(std::string(lists[i].first) + ": " + parsed.error);
            cameraunlock::input::RegisterKeyBindings(*poller, parsed.bindings, actions[i]);
            LogBindings(text, context, lists[i].first, *lists[i].second);
        }
        poller->Start(10);
        return 0;
    });
}

}  // extern "C"
