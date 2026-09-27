{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
      ...
    }:
    let
      inherit (nixpkgs) lib;
      eachSystem = lib.genAttrs lib.systems.flakeExposed;
    in
    {
      checks = eachSystem (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          reuse = pkgs.runCommand "abus-reuse-lint" { nativeBuildInputs = [ pkgs.reuse ]; } ''
            reuse --root ${self} lint
            touch $out
          '';
        }
      );

      devShells = eachSystem (
        system:
        let
          overlays = [ (import rust-overlay) ];
          pkgs = import nixpkgs {
            inherit system overlays;
          };
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              cargo-nextest
              cargo-edit
              cargo-expand
              cargo-bloat
              cargo-fuzz
              reuse
              (rust-bin.stable.latest.default.override {
                extensions = [
                  "rust-src"
                  "rust-analyzer"
                ];
              })
            ];
          };
        }
      );
    };
}
