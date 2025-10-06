{
  description = "pijul, the sound distributed version control system";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-compat.url = "https://flakehub.com/f/edolstra/flake-compat/1.tar.gz";
  };

  outputs =
    { self
    , nixpkgs
    , ...
    }:
    let
      nameValuePair = name: value: { inherit name value; };
      genAttrs = names: f: builtins.listToAttrs (map (n: nameValuePair n (f n)) names);
      forAllSystems = f: genAttrs allSystems (system: f nixpkgs.legacyPackages.${system});
      allSystems = [ "x86_64-linux" "aarch64-linux" "i686-linux" "x86_64-darwin" "aarch64-darwin" ];
      cargoMeta = builtins.fromTOML (builtins.readFile ./pijul/Cargo.toml);
    in {
      devShell = forAllSystems
        (pkgs:
          (pkgs.mkShell.override { stdenv = pkgs.clangStdenv; })
          {
            name = "pijul";

            inputsFrom = [ self.packages.${pkgs.system}.pijul-git ];

            packages = with pkgs; [
              rust-analyzer
              rustfmt
            ];

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang}/lib";
          }
        );

      packages = forAllSystems
        (pkgs: rec {
          default = pijul;

          pijul = pkgs.clangStdenv.mkDerivation (self: {
            pname = cargoMeta.package.name;
            version = cargoMeta.package.version;

            src = ./.;
            buildAndTestSubdir = "pijul";

            doCheck = true;
            cargoBuildType = "release";

            cargoDeps = pkgs.rustPlatform.importCargoLock {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = builtins.attrValues {
              inherit (pkgs)
                cargo
                libiconv
                pkg-config
                rustc
                ;
            };

            buildInputs = builtins.attrValues (
              {
                inherit (pkgs)
                  libsodium
                  openssl
                  ;

                inherit (pkgs.rustPlatform)
                  cargoBuildHook
                  cargoInstallHook
                  cargoSetupHook
                  ;
              }
              // nixpkgs.lib.optionalAttrs (pkgs.stdenv.isDarwin) {
                inherit (pkgs.darwin.apple_sdk.frameworks)
                  SystemConfiguration
                  ;
              }
            );
          });

          pijul-git = pijul.overrideAttrs (self: {
            cargoBuildFeatures = [ "git" ];
          });
        });
    };
}
