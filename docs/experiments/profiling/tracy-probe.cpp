// Measures a real Tracy client's disconnected state. No Aizome/GPU workload.
#include <tracy/Tracy.hpp>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <thread>

static inline void stage(uint64_t &value) {
#if defined(TRACY_MANUAL_LIFETIME)
    if (TracyIsStarted) {
        ZoneScopedN("interaction-stage");
        value = value * 6364136223846793005ULL + 1;
        asm volatile("" : "+r"(value));
        return;
    }
#else
    ZoneScopedN("interaction-stage");
#endif
    value = value * 6364136223846793005ULL + 1;
    asm volatile("" : "+r"(value));
}

int main(int argc, char **argv) {
    if (argc > 1 && std::strcmp(argv[1], "idle") == 0) {
        std::puts("ready");
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::seconds(60));
        return 0;
    }
    uint64_t value = 42;
    constexpr int count = 10000000;
    // Repeat inside the process; startup is intentionally outside these timings.
    for (int run = 0; run < 9; ++run) {
        auto start = std::chrono::steady_clock::now();
        for (int i = 0; i < count; ++i) stage(value);
        auto ns = std::chrono::duration_cast<std::chrono::nanoseconds>(
            std::chrono::steady_clock::now() - start).count();
        std::printf("%.6f\n", double(ns) / count);
    }
    return value == 0;
}
