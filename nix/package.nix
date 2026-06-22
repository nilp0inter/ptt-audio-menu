{
  lib,
  rustPlatform,
  stdenv,
  pkg-config,
  cmake,
  espeak-ng,
  llvmPackages,
  onnxruntime,
  openssl,
  sonic,
  alsa-lib,
  dbus,
  glibc,
}:

let
  isDarwin = stdenv.hostPlatform.isDarwin;

  commonBuildInputs = [
    espeak-ng
    onnxruntime
    openssl
    sonic
  ];

  linuxBuildInputs = commonBuildInputs ++ [
    alsa-lib
    dbus
  ];

  buildInputs = if isDarwin then commonBuildInputs else linuxBuildInputs;

  nativeBuildInputs = [
    cmake
    pkg-config
  ];

  commonAttrs = {
    pname = "ptt-audio-menu";
    version = "0.1.0";

    src = lib.cleanSource ../.;

    cargoLock.lockFile = ../Cargo.lock;

    inherit nativeBuildInputs buildInputs;

    LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
    PIPER_ESPEAKNG_DATA_DIRECTORY = "${espeak-ng}/share";
    RUSTFLAGS = "-C link-arg=-lsonic";
  };

  linuxAttrs = lib.optionalAttrs (!isDarwin) {
    BINDGEN_EXTRA_CLANG_ARGS = "-I${glibc.dev}/include";
  };

  darwinAttrs = lib.optionalAttrs isDarwin {
    LIBRARY_PATH = "${sonic}/lib:${onnxruntime}/lib";
  };
in

rustPlatform.buildRustPackage (commonAttrs // linuxAttrs // darwinAttrs // {
  meta = {
    description = "Screenless audio menu for a Bluetooth remote speaker microphone";
    mainProgram = "ptt-audio-menu";
    license = lib.licenses.mit;
    platforms = with lib.platforms; linux ++ darwin;
  };
})