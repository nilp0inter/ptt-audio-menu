{
  lib,
  rustPlatform,
  pkg-config,
  cmake,
  alsa-lib,
  dbus,
  espeak-ng,
  llvmPackages,
  onnxruntime,
  openssl,
  glibc,
  sonic,
}:

rustPlatform.buildRustPackage {
  pname = "ptt-audio-menu";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [
    cmake
    pkg-config
  ];

  buildInputs = [
    alsa-lib
    dbus
    espeak-ng
    onnxruntime
    openssl
    sonic
  ];

  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  BINDGEN_EXTRA_CLANG_ARGS = "-I${glibc.dev}/include";
  PIPER_ESPEAKNG_DATA_DIRECTORY = "${espeak-ng}/share";
  RUSTFLAGS = "-C link-arg=-lsonic";

  meta = {
    description = "Screenless audio menu for a Bluetooth remote speaker microphone";
    mainProgram = "ptt-audio-menu";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
