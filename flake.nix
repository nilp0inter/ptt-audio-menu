{
  description = "Development shell for ptt-audio-menu";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              cargo
              cmake
              rustc
              rustfmt
              clippy
              gcc
              pkg-config
              alsa-lib.dev
              dbus.dev
              espeak-ng
              llvmPackages.libclang.lib
              openssl.dev
            ];

            PKG_CONFIG_PATH = "${pkgs.alsa-lib.dev}/lib/pkgconfig:${pkgs.dbus.dev}/lib/pkgconfig:${pkgs.openssl.dev}/lib/pkgconfig";
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = "-I${pkgs.glibc.dev}/include";
            PIPER_ESPEAKNG_DATA_DIRECTORY = "${pkgs.espeak-ng}/share/espeak-ng-data";
          };
        });
    };
}
