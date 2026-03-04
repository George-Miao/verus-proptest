{
  description = "ThinCell dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?rev=c6245e83d836d0433170a16eb185cefe0572f8b8";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    verus-flake = {
      url = "github:stephen-huan/verus-flake";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.rust-overlay.follows = "rust-overlay";
    };
  };

  outputs =
    {
      nixpkgs,
      verus-flake,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        verusPkgs = verus-flake.packages.${system};
        verusfmtSrc = pkgs.fetchzip {
          url = "https://github.com/verus-lang/verusfmt/releases/download/v0.6.1/verusfmt-x86_64-unknown-linux-gnu.tar.xz";
          sha256 = "sha256-jphEd4BFOlVnj886UsrIxdLffCNrgzZnLvNpbKk42C8=";
        };
        verusfmtBin = pkgs.stdenv.mkDerivation {
          name = "verusfmt";
          src = verusfmtSrc;
          installPhase = ''
            mkdir -p $out/bin
            cp verusfmt $out/bin
          '';
        };
        verusfmt = pkgs.writeShellScriptBin "verusfmt" ''
          tmp=$(mktemp --suffix=.rs)
          cat > "$tmp"
          ${verusfmtBin}/bin/verusfmt "$tmp"
          cat "$tmp"
        '';
      in
      with pkgs;
      {
        devShells.default = mkShell {
          buildInputs = [
            verusPkgs.verus
            verusPkgs.rustup
            verusfmt
            bacon
            cargo-expand
            (rust-bin.selectLatestNightlyWith (
              toolchain:
              toolchain.default.override {
                extensions = [ "rust-src" ];
              }
            ))
          ];
        };
      }
    );
}
