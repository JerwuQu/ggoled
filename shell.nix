with import <nixpkgs> { };
mkShell {
  name = "ggoled-env";
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
