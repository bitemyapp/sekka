fn main() {
    // The cores' log function (src/retro/log.c): printf's arguments.
    cc::Build::new().file("src/retro/log.c").opt_level(2).compile("chanoma_core_log");
    println!("cargo:rerun-if-changed=src/retro/log.c");
}
