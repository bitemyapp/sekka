# The C libraries and native tools Aizome's package builds with
# (desktop/aizome/nix/package.nix), for Bazel's native environment
# (toolchains.nix). The flake's `bazel-native-inputs` check compares this list
# with the package's own, so the two can't drift apart.
{ pkgs }:
{
  buildInputs = with pkgs; [
    wayland
    libxkbcommon
    libinput
    udev
    seatd
    libdrm
    libgbm
    libglvnd
    mesa
    pixman
    fontconfig
    freetype
    dbus
    pam
    libpulseaudio
    (import ../libsecret-dh-padding.nix { inherit (pkgs) lib libsecret; })
    ffmpeg
    pipewire
    openblas
    vulkan-headers
    vulkan-loader
  ];
  # Koe's whisper.cpp: CMake, run by whisper-rs-sys, and glslc for its
  # Vulkan shaders. pkg-config and bindgen's hook come with toolchains.nix.
  nativeBuildInputs = with pkgs; [
    cmake
    shaderc
  ];
  # The package's settings for whisper.cpp: OpenBLAS picks kernels at run
  # time, the rest stays portable (no -march).
  env = {
    BLAS_INCLUDE_DIRS = "${pkgs.lib.getDev pkgs.openblas}/include";
    OPENBLAS_PATH = "${pkgs.lib.getLib pkgs.openblas}";
    GGML_NATIVE = "OFF";
    GGML_AVX = "OFF";
    GGML_AVX2 = "OFF";
    GGML_FMA = "OFF";
    GGML_F16C = "OFF";
  };
}
