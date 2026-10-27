//! `cargo run -p aizome-shashin --example samples DIR`: write pictures to
//! try Shashin with: every kind it reads, and a 24-megapixel camera JPEG
//! with its EXIF thumbnail.

#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "shashin-samples".into()));
    std::fs::create_dir_all(&dir).unwrap();
    common::write_folder(&dir);
    std::fs::write(dir.join("DSCF0042.jpg"), common::camera_jpeg(6000, 4000)).unwrap();
    println!("wrote samples to {}", dir.display());
}
