fn main() {
    let secret = pkg_config::probe_library("libsecret-1").expect("libsecret development files");
    let mut build = cc::Build::new();
    build.file("src/keyring.c").opt_level(2);
    for path in secret.include_paths {
        build.include(path);
    }
    build.compile("aizome_network_keyring");
    println!("cargo:rerun-if-changed=src/keyring.c");
}
