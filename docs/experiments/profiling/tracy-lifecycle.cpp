#include <tracy/Tracy.hpp>
#include <chrono>
#include <cstdio>
#include <thread>

int main() {
    for (int i = 0; i < 2; ++i) {
        tracy::StartupProfiler();
        {
            ZoneScopedN("before-shutdown");
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
        }
        tracy::ShutdownProfiler();
    }
    std::puts("ready");
    std::fflush(stdout);
    std::this_thread::sleep_for(std::chrono::seconds(60));
}
