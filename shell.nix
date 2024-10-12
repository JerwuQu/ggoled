with import <nixpkgs> { };
mkShell {
  name = "ggoled-env";
  packages = [
    rustup
    gcc
    pkg-config
    udev

    # ggoled_app
    gtk3.dev
    # TODO: further deps
  ];
  shellHook = ''
    rustup default stable
  '';
}
