//! The binary finds the core's library beside it (`target/<profile>/deps`
//! in a Cargo build; the Nix package sets its own run path).

fn main() {
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN:$ORIGIN/deps:$ORIGIN/../lib/aizome");
}
