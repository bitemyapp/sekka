#!/usr/bin/env bash
# Functional checks on the disposable, disk-booted all-applications VM.
set -euo pipefail
test "$(hostname)" = rust-test
test "$(id -un)" = rusttest
test "$(id -u)" = 1000
test "$(findmnt -n -o FSTYPE /)" != tmpfs
test -f /etc/installer-applications.json

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"

cat > hello.c <<'EOF'
#include <stdio.h>
int main(void) { puts("C toolchain works"); return 0; }
EOF
cat > hello.cpp <<'EOF'
#include <iostream>
int main() { std::cout << "C++ toolchain works\n"; }
EOF
cat > Makefile <<'EOF'
all: hello-c hello-cpp
hello-c: hello.c
	$(CC) -Wall -Werror hello.c -o hello-c
hello-cpp: hello.cpp
	$(CXX) -Wall -Werror hello.cpp -o hello-cpp
EOF
make
test "$(./hello-c)" = 'C toolchain works'
test "$(./hello-cpp)" = 'C++ toolchain works'
cat > CMakeLists.txt <<'EOF'
cmake_minimum_required(VERSION 3.20)
project(installer_toolchain_test LANGUAGES C CXX)
add_executable(cmake-c hello.c)
add_executable(cmake-cpp hello.cpp)
EOF
cmake -S . -B build -G Ninja
cmake --build build
test "$(build/cmake-c)" = 'C toolchain works'
test "$(build/cmake-cpp)" = 'C++ toolchain works'
pkg-config --version
git init
git add hello.c
test "$(git diff --cached --name-only)" = hello.c
printf '%s\n' 'PASS C, C++, Make, CMake, Ninja and Git'

# A real freshly installed Rust toolchain, linking a compiled C library.
rustup default stable
rustup show
cargo new native-link
cd native-link
cat > answer.c <<'EOF'
int answer(void) { return 42; }
EOF
cat > build.rs <<'EOF'
use std::{env, process::Command};
fn main() {
    let out = env::var("OUT_DIR").unwrap();
    assert!(Command::new("cc").args(["-c", "answer.c", "-o"])
        .arg(format!("{out}/answer.o")).status().unwrap().success());
    assert!(Command::new("ar").arg("crs").arg(format!("{out}/libanswer.a"))
        .arg(format!("{out}/answer.o")).status().unwrap().success());
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=answer");
}
EOF
cat > src/main.rs <<'EOF'
unsafe extern "C" { fn answer() -> i32; }
fn main() { assert_eq!(unsafe { answer() }, 42); println!("Rust and C linkage works"); }
EOF
test "$(cargo run --quiet)" = 'Rust and C linkage works'
cargo test
printf '%s\n' 'PASS Rustup stable, Cargo and native C linkage'
cd "$work"

# Exercise rootless networking, execution and the Compose plugin as the user.
export XDG_RUNTIME_DIR="/run/user/$(id -u)"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
test "${DOCKER_HOST:-}" = "unix://$XDG_RUNTIME_DIR/docker.sock"
systemctl --user is-active docker
if id -nG | tr ' ' '\n' | grep -qx docker; then
    echo 'Unexpected privileged docker group membership' >&2
    exit 1
fi
docker info --format '{{json .SecurityOptions}}' | grep -q rootless
docker compose version
cat > compose.yaml <<'EOF'
services:
  smoke:
    image: busybox:1.37.0
    command: ["sh", "-ec", "test $$(id -u) = 0; echo rootless-compose-works"]
EOF
docker compose --project-name installer-smoke up --abort-on-container-exit --exit-code-from smoke
docker compose --project-name installer-smoke logs --no-color | grep -q rootless-compose-works
docker image inspect busybox:1.37.0 --format '{{json .RepoDigests}}'
docker compose --project-name installer-smoke down
printf '%s\n' 'PASS rootless Docker and Compose'

codex --version
claude --version
omp --version
opencode --version
nvim --version
lazygit --version
printf '%s\n' 'PASS terminal application startup'
