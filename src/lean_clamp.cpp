#include "cameraunlock/camera/lean_clamp.h"

namespace {

struct LeanHit {
    int queried;
    int blocked;
    float distance;
};

typedef void (*LeanQuery)(void* context, const float* start, const float* direction, float max_distance,
                          LeanHit* out);

struct Query {
    LeanQuery query;
    void* context;
};

thread_local cameraunlock::camera::LeanClamp clamp;

cameraunlock::camera::LeanObstruction Ask(void* context, const cameraunlock::math::Vec3& start,
                                          const cameraunlock::math::Vec3& direction, float max_distance) {
    const Query& query = *static_cast<const Query*>(context);
    const float s[3] = {start.x, start.y, start.z};
    const float d[3] = {direction.x, direction.y, direction.z};
    LeanHit hit{};
    query.query(query.context, s, d, max_distance, &hit);
    return {hit.queried != 0, hit.blocked != 0, hit.distance};
}

}  // namespace

extern "C" void reset_lean_clamp() { clamp.Reset(); }

// The query holds the eye off surfaces itself (a box as wide as the standoff), so the
// policy's skin is 0: a second skin would apply the standoff twice. Returns bit 0 for
// InContact and bit 1 for LastQueryFailed.
extern "C" int apply_lean_clamp(float* offset, const float* eye, float delta_time, float release_smoothing,
                                LeanQuery query, void* context) {
    cameraunlock::camera::LeanClampSettings settings;
    settings.skin = 0.0f;
    settings.release_smoothing = release_smoothing;
    clamp.SetSettings(settings);
    Query bridge{query, context};
    const auto result = clamp.Apply({eye[0], eye[1], eye[2]}, {offset[0], offset[1], offset[2]}, delta_time,
                                    Ask, &bridge);
    offset[0] = result.x;
    offset[1] = result.y;
    offset[2] = result.z;
    return (clamp.InContact() ? 1 : 0) | (clamp.LastQueryFailed() ? 2 : 0);
}
