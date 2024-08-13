with import <nixpkgs> { };
mkShell {
  name = "sanpwo-env";
  packages = [
    rustup
    pkgsCross.mingwW64.stdenv.cc
  ];
  shellHook = ''
    export CC=x86_64-w64-mingw32-gcc
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-L native=${pkgs.pkgsCross.mingwW64.windows.pthreads}/lib"
    export CARGO_BUILD_TARGET=x86_64-pc-windows-gnu
    rustup default stable
    rustup target add x86_64-pc-windows-gnu
  '';
}
