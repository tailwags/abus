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

          # The license texts are copied next to what they cover so that GitHub
          # and crates.io can find them; keep the copies identical to LICENSES/.
          license-copies = pkgs.runCommand "abus-license-copies" { } ''
            cd ${self}
            cmp LICENSES/Apache-2.0.txt LICENSE-APACHE
            cmp LICENSES/Apache-2.0.txt crates/abus/LICENSE
            cmp LICENSES/EUPL-1.2.txt LICENSE-EUPL
            cmp LICENSES/EUPL-1.2.txt crates/abusd/LICENSE
            cmp LICENSES/EUPL-1.2.txt crates/abusctl/LICENSE
            cmp LICENSES/GPL-2.0-or-later.txt docs/COPYING
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
