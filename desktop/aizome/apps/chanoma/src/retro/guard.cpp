// A core's calls with their C++ exceptions caught. A core written in C++
// that lets one out (Play!'s retro_run, when it can't boot a disc) would
// otherwise unwind into Chanoma's Rust, which aborts on a foreign
// exception. Caught here, with no Rust between the core and the catch,
// the game ends with what the exception said (guard.rs).
//
// Each returns 1 if the call returned, 0 if it threw (what it said in
// `why`).

#include <cstddef>
#include <cstring>
#include <exception>

namespace {

void say(char *why, std::size_t room, const char *what)
{
    if (room == 0)
        return;
    std::strncpy(why, what, room - 1);
    why[room - 1] = '\0';
}

template <typename Call> int guarded(Call call, char *why, std::size_t room)
{
    try {
        call();
        return 1;
    } catch (const std::exception &e) {
        say(why, room, e.what());
    } catch (...) {
        say(why, room, "an exception");
    }
    return 0;
}

} // namespace

// retro_run, retro_reset, retro_unload_game, retro_deinit, and a context's
// reset and destroy.
extern "C" int chanoma_guard_void(void (*f)(void), char *why, std::size_t room)
{
    return guarded([&] { f(); }, why, room);
}

// retro_load_game.
extern "C" int chanoma_guard_load(bool (*f)(const void *), const void *game, bool *ok, char *why, std::size_t room)
{
    return guarded([&] { *ok = f(game); }, why, room);
}

// retro_serialize and retro_unserialize.
extern "C" int chanoma_guard_state(bool (*f)(void *, std::size_t), void *data, std::size_t size, bool *ok, char *why,
                                   std::size_t room)
{
    return guarded([&] { *ok = f(data, size); }, why, room);
}
