#!/usr/bin/env sh
set -eu

if ! command -v apt-get >/dev/null 2>&1; then
    echo "This bootstrap script currently supports Ubuntu/Debian hosts only." >&2
    exit 1
fi

# Fail before sudo on distributions whose standard native APIs are too old.
if [ -r /etc/os-release ]; then
    . /etc/os-release
    if [ "${ID:-}" = ubuntu ] && ! dpkg --compare-versions "${VERSION_ID:-0}" ge 26.04; then
        echo 'Use Ubuntu 26.04 or newer; standard Ubuntu 24.04 cannot build the required GTK APIs.' >&2
        exit 1
    fi
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
    python3 \
    rustc

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python3 "$project_root/scripts/check-requirements.py"

echo "Toolchain ready:"
rustc --version
cargo --version
pkg-config --modversion gtk4
pkg-config --modversion libadwaita-1
