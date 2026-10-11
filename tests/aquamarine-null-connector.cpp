// Exercise the actual shared library without opening a seat, GPU or display.
// -fno-access-control lets this regression test construct an inert backend and
// reproduce the cleared connector slots encountered during DRM teardown.
#include <aquamarine/backend/DRM.hpp>
#include <aquamarine/backend/Session.hpp>

using namespace Aquamarine;
using namespace Hyprutils::Memory;

int main() {
    auto backend = CSharedPointer<CBackend>(new CBackend());
    backend->session = makeShared<CSession>();
    CDRMBackend drm(backend);

    drm.flushAsyncCommitEvents();
    drm.connectors = {nullptr, nullptr};
    drm.flushAsyncCommitEvents(); // Unpatched 0.15.1 segfaults here.
    drm.connectors.clear();      // No hardware connectors to disconnect.
}
