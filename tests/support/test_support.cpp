// Test-only entry points into cameraunlock-core for the Rust tests. build.rs compiles this
// file only with the test-support feature, so none of it reaches the shipped DLL.

#include <cstddef>
#include <exception>
#include <string>
#include <vector>

#include "cameraunlock/config/legacy_import.h"
#include "cameraunlock/config/testing/ini_mutations.h"

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

}  // extern "C"
