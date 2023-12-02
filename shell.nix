with import <nixpkgs> { };
mkShell {
  name = "sanpwo-env";
  packages = [
    rustup
    gcc
    pkg-config
    udev
  ];
  shellHook = ''
    rustup default stable
  '';
}
