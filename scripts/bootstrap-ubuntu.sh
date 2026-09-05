#!/usr/bin/env sh
set -eu

if ! command -v apt-get >/dev/null 2>&1; then
    echo "This bootstrap script currently supports Ubuntu/Debian hosts only." >&2
    exit 1
fi

echo "Installing the native build toolchain (the launcher itself never runs as root)..."
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    cargo \
    libadwaita-1-dev \
    libgtk-4-dev \
    libsqlite3-dev \
    pkg-config \
    rustc

echo "Toolchain ready:"
rustc --version
cargo --version
pkg-config --modversion gtk4
pkg-config --modversion libadwaita-1
