{
  description = "wasmer Webassembly runtime";

  inputs = {
    flakeutils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flakeutils }:
    flakeutils.lib.eachDefaultSystem (system:
      let
        NAME = "wasmer";
        VERSION = "0.1";

        pkgs = import nixpkgs {
          inherit system;
        };

      in
      rec {

        # packages.${NAME} = pkgs.stdenv.mkDerivation {
        #   pname = NAME;
        #   version = VERSION;

        #   buildPhase = "echo 'no-build'";
        # };

        # defaultPackage = packages.${NAME};

        # # For `nix run`.
        # apps.${NAME} = flakeutils.lib.mkApp {
        #   drv = packages.${NAME};
        # };
        # defaultApp = apps.${NAME};

        devShell = pkgs.stdenv.mkDerivation {
          name = NAME;
          src = self;
          buildInputs = with pkgs; [
            pkgconfig
            openssl
            llvmPackages_15.libllvm
            # Snapshot testing
            cargo-insta
            wabt
            binaryen

            # LLVM and related dependencies
            llvmPackages_15.llvm
            libxml2
            libffi

            # Test runner
            cargo-nextest
          ];
          runtimeDependencies = with pkgs; [ ];

          LD_LIBRARY_PATH = "${pkgs.openssl.out}/lib";
          LLVM_SYS_150_PREFIX = "${pkgs.llvmPackages_15.llvm.dev}";
        };
      }
    );
}
