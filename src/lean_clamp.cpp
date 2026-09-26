#include "cameraunlock/camera/lean_clamp.h"

static thread_local cameraunlock::camera::LeanClamp clamp;

extern "C" void reset_lean_clamp() {
    clamp.Reset();
}

extern "C" void apply_lean_clamp(float* offset, float delta_time, float skin,
                                float release_smoothing, int blocked, float distance) {
    cameraunlock::camera::LeanClampSettings settings;
    settings.skin = skin;
    settings.release_smoothing = release_smoothing;
    clamp.SetSettings(settings);
    cameraunlock::camera::LeanObstruction hit{true, blocked != 0, distance};
    const auto query = [](void* context, const cameraunlock::math::Vec3&,
                          const cameraunlock::math::Vec3&, float) {
        return *static_cast<cameraunlock::camera::LeanObstruction*>(context);
    };
    const auto result = clamp.Apply({}, {offset[0], offset[1], offset[2]},
                                    delta_time, query, &hit);
    offset[0] = result.x;
    offset[1] = result.y;
    offset[2] = result.z;
}
