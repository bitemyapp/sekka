use std::{env, fs, path::PathBuf};
fn main() {
    assert_eq!(
        env::var("CARGO_CFG_TARGET_OS").unwrap(),
        "linux",
        "this dependency surface is Linux-only"
    );
    let width = env::var("CARGO_CFG_TARGET_POINTER_WIDTH").unwrap();
    let word = match width.as_str() {
        "64" => 8,
        "32" => 4,
        _ => panic!("unsupported Linux C ABI"),
    };
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut config = String::from(
        "#define PACKAGE \"oniguruma\"\n#define PACKAGE_VERSION \"6.9.9\"\n#define VERSION \"6.9.9\"\n",
    );
    for name in [
        "HAVE_ALLOCA",
        "HAVE_ALLOCA_H",
        "HAVE_STDINT_H",
        "HAVE_SYS_TIMES_H",
        "HAVE_SYS_TIME_H",
        "HAVE_SYS_TYPES_H",
        "HAVE_UNISTD_H",
        "HAVE_INTTYPES_H",
    ] {
        config.push_str(&format!("#define {name} 1\n"));
    }
    config.push_str(&format!("#define SIZEOF_INT 4\n#define SIZEOF_LONG {word}\n#define SIZEOF_LONG_LONG 8\n#define SIZEOF_VOIDP {word}\n"));
    fs::write(output.join("config.h"), config).unwrap();
    let source = PathBuf::from("vendor/oniguruma/src");
    let mut build = cc::Build::new();
    build.include(&source).include(output).warnings(false);
    for file in include_str!("vendor/oniguruma/sources.txt").lines() {
        build.file(source.join(file));
    }
    build.file("src/abi.c");
    build.compile("hotaru_oniguruma_699");
    println!("cargo:rerun-if-changed=vendor/oniguruma");
    println!("cargo:rerun-if-changed=src/abi.c");
}
