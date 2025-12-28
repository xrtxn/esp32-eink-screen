{
  pkgs ? import <nixpkgs> { },
}:
let
  # Use the latest release of esp-rs-nix
  esp-rs-src = builtins.fetchTarball "https://github.com/leighleighleigh/esp-rs-nix/archive/main.tar.gz";

  # Call the package from the fetched source
  esp-rs = pkgs.callPackage "${esp-rs-src}/esp-rs/default.nix" { };
in
pkgs.mkShell {
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
}
