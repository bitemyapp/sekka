fn main() {
    let avutil = pkg_config::Config::new().probe("libavutil").expect("FFmpeg's libavutil");
    cc::Build::new().file("src/ffmpeg_log.c").includes(avutil.include_paths).compile("aizome_ffmpeg_log");
    println!("cargo:rerun-if-changed=src/ffmpeg_log.c");
}
