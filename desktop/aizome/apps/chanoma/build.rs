fn main() {
    // The cores' log function (src/retro/log.c): printf's arguments.
    cc::Build::new().file("src/retro/log.c").opt_level(2).compile("chanoma_core_log");
    println!("cargo:rerun-if-changed=src/retro/log.c");
    // Calls into cores with their C++ exceptions caught (src/retro/guard.cpp),
    // linked with the system's C++ runtime, as the cores that throw are.
    cc::Build::new().cpp(true).file("src/retro/guard.cpp").opt_level(2).compile("chanoma_guard");
    println!("cargo:rerun-if-changed=src/retro/guard.cpp");
}
