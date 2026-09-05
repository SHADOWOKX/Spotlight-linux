#!/usr/bin/env python3
"""Exercise GApplication commands, not physical shortcut events.

Run on a private test bus/display, or explicitly pass the installed binary on
the real desktop. Leaves the same resident instance running and hidden.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument("binary", help="absolute launcher executable path")
args = parser.parse_args()
owner = subprocess.run([
    "gdbus", "call", "--session", "--dest", "org.freedesktop.DBus",
    "--object-path", "/org/freedesktop/DBus", "--method",
    "org.freedesktop.DBus.NameHasOwner", "io.github.shadowokx.SpotlightLinux",
], check=True, capture_output=True, text=True, timeout=5)
if "true" not in owner.stdout:
    raise SystemExit("Open Spotlight Linux first, then run this resident-lifecycle check.")


def command(*options):
    return subprocess.run([args.binary, *options], check=True, text=True,
                          capture_output=True, timeout=10).stdout


def snapshot():
    return dict(line.split("=", 1) for line in command("--diagnostics").splitlines()
                if "=" in line)


initial = snapshot()
pid = initial["pid"]
command("--hide")
assert snapshot()["visible"] == "false"
for _ in range(20):
    command("--toggle")
    # Visibility on a real compositor may immediately change on focus loss.
    # Require identity/window stability, not a claim about compositor focus.
    current = snapshot()
    assert current["pid"] == pid and current["windows"] == "1", current
    command("--hide")
    assert snapshot()["visible"] == "false"

with ThreadPoolExecutor(max_workers=8) as executor:
    list(executor.map(lambda _: command("--background"), range(20)))
assert snapshot()["pid"] == pid
command("--settings")
windows = snapshot()["windows"]
for _ in range(5):
    command("--settings")
    assert snapshot()["windows"] == windows
command("--hide")
time.sleep(0.3)  # Allow native dialog close animations; this is a test only.
final = snapshot()
assert final["pid"] == pid and final["windows"] == "1", final
assert final["resident"] == "true" and final["visible"] == "false", final
print(f"PASS: PID {pid}; 20 toggle/hide cycles, 20 concurrent background invocations, "
      "5 repeated Settings activations; one resident launcher remains hidden.")
