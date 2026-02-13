{
  description = "Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      treefmt-nix,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src"
            "clippy"
            "rust-analyzer"
          ];
        };
        textlintrc = (pkgs.formats.json { }).generate "textlintrc" {
          plugins = {
            "@textlint/markdown" = true;
          };
          rules = {
            preset-ja-technical-writing = {
              ja-no-mixed-period = false;
              no-exclamation-question-mark = false;
            };
            write-good = true;
            prh.rulePaths = [
              "${pkgs.textlint-rule-prh}/lib/node_modules/textlint-rule-prh/node_modules/prh/prh-rules/media/techbooster.yml"
              "${pkgs.textlint-rule-prh}/lib/node_modules/textlint-rule-prh/node_modules/prh/prh-rules/media/WEB+DB_PRESS.yml"
            ];
          };
        };
        treefmtEval = treefmt-nix.lib.evalModule pkgs {
          projectRootFile = "flake.nix";
          programs.nixfmt.enable = true;
          programs.rustfmt.enable = true;
          programs.taplo.enable = true;
        };
      in
      {
        formatter = treefmtEval.config.build.wrapper;
        checks.formatting = treefmtEval.config.build.check;
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            just
            (textlint.withPackages [
              textlint-rule-preset-ja-technical-writing
              textlint-rule-prh
              textlint-rule-write-good
              "@textlint/markdown"
            ])
          ];

          shellHook = ''
            [ -f .textlintrc ] && unlink .textlintrc
            ln -s ${textlintrc} .textlintrc
          '';
        };
      }
    );
}
