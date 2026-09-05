#!/usr/bin/env python3
"""Build release binaries without embedding the builder's home or checkout path."""
import os
from pathlib import Path
import shlex
import subprocess

root = Path(__file__).resolve().parents[1]
subprocess.run(["python3", str(root / "scripts/check-requirements.py")], check=True)
env = os.environ.copy()
flags = env.get("CARGO_ENCODED_RUSTFLAGS", "").split("\x1f") if env.get("CARGO_ENCODED_RUSTFLAGS") else shlex.split(env.get("RUSTFLAGS", ""))
# Specific paths are mapped after HOME; rustc uses the last matching mapping.
mappings = [(Path.home(), "/build/user"), (Path(env.get("CARGO_HOME", Path.home() / ".cargo")).resolve(), "/build/cargo"), (root, "/usr/src/spotlight-linux")]
flags.extend(f"--remap-path-prefix={source}={target}" for source, target in mappings)
env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
subprocess.run(["cargo", "build", "--manifest-path", str(root / "Cargo.toml"), "--target-dir", str(root / "target"), "--locked", "--release", "-p", "spotlight-gtk"], env=env, check=True)
