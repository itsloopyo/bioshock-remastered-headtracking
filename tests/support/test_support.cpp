// Test-only entry points into cameraunlock-core for the Rust tests. build.rs compiles this
// file only with the test-support feature, so none of it reaches the shipped DLL.

#include <cstddef>
#include <exception>
#include <string>
#include <vector>

#include "cameraunlock/config/legacy_import.h"
#include "cameraunlock/config/testing/ini_mutations.h"
#include "cameraunlock/input/key_bindings.h"

#include "config_owner.h"

extern "C" {

struct BsrLegacyKey {
    const char* section;
    const char* key;
};

struct BsrMutationKey {
    const char* section;
    const char* key;
    const char* alternate;
    const char* const* out_of_range;
    std::size_t out_of_range_len;
    int hotkey;
};

typedef void (*BsrEmitPair)(void* context, const char* name, std::size_t name_len, const char* bytes,
                            std::size_t bytes_len);
typedef void (*BsrEmitText)(void* context, const char* text, std::size_t len);

// GenerateIniMutations over `base`: one emit per corpus input. Returns 0, or 1 with the
// generator's refusal handed to `error`.
int bsr_test_ini_mutations(const char* base, std::size_t base_len, const BsrLegacyKey* reads, std::size_t reads_len,
                           const BsrMutationKey* keys, std::size_t keys_len, BsrEmitPair emit, BsrEmitText error,
                           void* context) {
    try {
        std::vector<cameraunlock::config::LegacyKey> read_keys;
        for (std::size_t i = 0; i < reads_len; ++i) read_keys.push_back({reads[i].section, reads[i].key});
        std::vector<cameraunlock::config::testing::MutationKey> mutation_keys;
        for (std::size_t i = 0; i < keys_len; ++i) {
            cameraunlock::config::testing::MutationKey k;
            k.section = keys[i].section;
            k.key = keys[i].key;
            k.alternate = keys[i].alternate;
            for (std::size_t j = 0; j < keys[i].out_of_range_len; ++j) k.out_of_range.emplace_back(keys[i].out_of_range[j]);
            k.hotkey = keys[i].hotkey != 0;
            mutation_keys.push_back(std::move(k));
        }
        for (const auto& m : cameraunlock::config::testing::GenerateIniMutations(std::string(base, base_len),
                                                                                 read_keys, mutation_keys)) {
            emit(context, m.name.data(), m.name.size(), m.bytes.data(), m.bytes.size());
        }
        return 0;
    } catch (const std::exception& e) {
        const std::string what = e.what();
        error(context, what.data(), what.size());
        return 1;
    }
}

// The mod's owner over the Defaults.ini at `defaults`, a scratch file, so no test reads or
// creates the player's own. Null after an exception, whose text goes to `text`.
BsrConfigOwner* bsr_test_config_owner_new_at(const wchar_t* folder, std::size_t folder_len, const wchar_t* defaults,
                                             std::size_t defaults_len, BsrLegacyReader reader, BsrText text,
                                             void* context) {
    try {
        return new BsrConfigOwner(BioShockRemasteredHeadTracking::ConfigOptions(
            std::wstring(folder, folder_len),
            cameraunlock::config::DefaultsFile::At(std::wstring(defaults, defaults_len)), reader));
    } catch (const std::exception& e) {
        const std::string what = e.what();
        text(context, 2, what.data(), what.size());
        return nullptr;
    }
}

// The file the owner creates where there is none, and the committed file.
void bsr_test_render_fresh(BsrEmitText emit, void* context) {
    const std::string bytes = cameraunlock::config::RenderCanonicalFresh(BioShockRemasteredHeadTracking::ConfigTable(),
                                                                         BioShockRemasteredHeadTracking::ConfigHeader());
    emit(context, bytes.data(), bytes.size());
}

// Every setting the owner's last Load gave, written as the renderer writes each, so two loads
// compare whole.
void bsr_test_render_loaded(const BsrConfigOwner* owner, BsrEmitText emit, void* context) {
    const std::string bytes = cameraunlock::config::RenderCanonical(BioShockRemasteredHeadTracking::ConfigTable(),
                                                                    owner->loaded,
                                                                    BioShockRemasteredHeadTracking::ConfigHeader());
    emit(context, bytes.data(), bytes.size());
}

typedef void (*BsrEmitBinding)(void* context, std::uint32_t modifiers, std::int32_t vk);

// The bindings the mod registers for one list of the last Load: 0 ToggleKey, 1
// CycleTrackingModeKey, 2 YawModeKey. Returns 0, or 1 when the list does not parse.
int bsr_test_hotkey_bindings(const BsrConfigOwner* owner, int which, BsrEmitBinding emit, void* context) {
    const BioShockRemasteredHeadTracking::Config& c = owner->loaded;
    const std::string& list = which == 0 ? c.toggle_key : which == 1 ? c.cycle_tracking_mode_key : c.yaw_mode_key;
    const cameraunlock::input::KeyBindingsParseResult parsed = cameraunlock::input::ParseKeyBindings(list);
    if (!parsed.ok()) return 1;
    for (const cameraunlock::input::KeyBinding& b : parsed.bindings) {
        emit(context, static_cast<std::uint32_t>(b.modifiers), b.vk);
    }
    return 0;
}

// The keys the import names, section and key per emit.
void bsr_test_import_keys(BsrEmitPair emit, void* context) {
    for (const cameraunlock::config::LegacyKey& k : BioShockRemasteredHeadTracking::ConfigLegacyImport(nullptr).keys) {
        emit(context, k.section.data(), k.section.size(), k.key.data(), k.key.size());
    }
}

}  // extern "C"
