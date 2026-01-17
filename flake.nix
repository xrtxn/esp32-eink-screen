{
  description = "ESP32 thesis project using esp-rs-nix for Rust development";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    esp-rs-nix = {
      url = "github:leighleighleigh/esp-rs-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      esp-rs-nix,
      ...
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          esp-rs = esp-rs-nix.packages.${system}.esp-rs;
        in
        {
          default = pkgs.mkShell {
            name = "esp32-thesis";

            buildInputs = [
              esp-rs
              pkgs.rustup
              pkgs.espflash
              pkgs.rust-analyzer
              pkgs.pkg-config
              pkgs.stdenv.cc
              pkgs.systemdMinimal
            ];

            shellHook = ''
              # Add a prefix to the shell prompt
              export PS1="(esp-rs)$PS1"

              # This variable is important - it tells rustup where to find the esp toolchain,
              # without needing to copy it into your local ~/.rustup/ folder.
              export RUSTUP_TOOLCHAIN=${esp-rs}

              # Set RUST_SRC_PATH for build-std to find the library sources
              export RUST_SRC_PATH=${esp-rs}/lib/rustlib/src/rust/library

              # Override where Cargo looks for stdlib sources (needed for build-std)
              export __CARGO_TESTS_ONLY_SRC_ROOT=${esp-rs}/lib/rustlib/src/rust/library
            '';
          };
        }
      );
    };
}
