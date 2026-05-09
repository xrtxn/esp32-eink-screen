{
  description = "ESP32 thesis project using esp-rs-nix for Rust development";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    esp-rs-nix = {
      url = "github:xrtxn/esp-rs-nix";
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

              pkgs.cargo-bloat
              pkgs.SDL2

              pkgs.bacon
              pkgs.pre-commit
              pkgs.esp-generate
            ];

            shellHook = ''
              # Add a prefix to the shell prompt
              export PS1="(esp-rs)$PS1"

              export AP_SSID="Thesis-MM"
              export AP_PASS="Thesis2026"

              # This variable is important - it tells rustup where to find the esp toolchain,
              # without needing to copy it into your local ~/.rustup/ folder.
              export RUSTUP_TOOLCHAIN=${esp-rs}

              # Set RUST_SRC_PATH for build-std to find the library sources
              export RUST_SRC_PATH=${esp-rs}/lib/rustlib/src/rust/library

              # Override where Cargo looks for stdlib sources (needed for build-std)
              export __CARGO_TESTS_ONLY_SRC_ROOT=${esp-rs}/lib/rustlib/src/rust/library

              # Fetch Pico CSS if not already present
              if [ ! -f web/static/pico.min.css ]; then
                mkdir -p web/static
                ${pkgs.curl}/bin/curl -fsSL \
                  https://cdn.jsdelivr.net/npm/@picocss/pico@2/css/pico.min.css \
                  -o web/static/pico.min.css
              fi
            '';
          };
        }
      );
    };
}
