#!/bin/bash
set -e
cd "$(dirname "$0")/.."

# Build reqs
git submodule update --init flatpak/shared-modules
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive flathub org.flatpak.Builder

# Vendor deps
cargo vendor flatpak/cargo-vendor > flatpak/cargo-config.toml

# Build
flatpak run org.flatpak.Builder --user --repo=flatpak/repo --force-clean --install-deps-from=flathub --state-dir=flatpak/.flatpak-builder flatpak/build-dir flatpak/se.ramse.ggoled.yml
flatpak build-bundle flatpak/repo flatpak/ggoled-"$(flatpak --default-arch)".flatpak se.ramse.ggoled
