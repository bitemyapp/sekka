// Standalone SDK experiment: two bounded captures in one process, with an
// unrecorded interval between them. These are synthetic spans, not app frames.
#include "perfetto.h"
#include <chrono>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <thread>

PERFETTO_DEFINE_CATEGORIES(perfetto::Category("aizome.probe"));
PERFETTO_TRACK_EVENT_STATIC_STORAGE();

static void initialize() {
    perfetto::TracingInitArgs args;
    args.backends = perfetto::kInProcessBackend;
    perfetto::Tracing::Initialize(args);
    perfetto::TrackEvent::Register();
}

static void stage() {
    TRACE_EVENT("aizome.probe", "synthetic-stage");
    std::this_thread::sleep_for(std::chrono::microseconds(100));
}

int main(int argc, char **argv) {
    if (argc > 1 && std::strcmp(argv[1], "lazy-idle") == 0) {
        std::puts("ready");
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::seconds(60));
        return 0;
    }
    initialize();
    if (argc > 1 && std::strcmp(argv[1], "initialized-idle") == 0) {
        std::puts("ready");
        std::fflush(stdout);
        std::this_thread::sleep_for(std::chrono::seconds(60));
        return 0;
    }
    for (int round = 0; round < 2; ++round) {
        perfetto::TraceConfig config;
        config.add_buffers()->set_size_kb(512);
        auto *source = config.add_data_sources()->mutable_config();
        source->set_name("track_event");
        perfetto::protos::gen::TrackEventConfig categories;
        categories.add_disabled_categories("*");
        categories.add_enabled_categories("aizome.probe");
        source->set_track_event_config_raw(categories.SerializeAsString());
        auto session = perfetto::Tracing::NewTrace();
        session->Setup(config);
        session->StartBlocking();
        for (int i = 0; i < 1000; ++i) stage();
        perfetto::TrackEvent::Flush();
        session->StopBlocking();
        auto trace = session->ReadTraceBlocking();
        std::ofstream file(std::string("capture-") + std::to_string(round) + ".pftrace", std::ios::binary);
        file.write(trace.data(), trace.size());
        std::printf("round=%d bytes=%zu\n", round, trace.size());
        // Must not appear in either capture.
        TRACE_EVENT_BEGIN("aizome.probe", "disabled-between-sessions");
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
        TRACE_EVENT_END("aizome.probe");
    }
    // Inspect teardown's actual idle footprint, separate from initial state.
    std::puts("ready");
    std::fflush(stdout);
    std::this_thread::sleep_for(std::chrono::seconds(60));
}
