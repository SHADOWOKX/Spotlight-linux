#!/usr/bin/env python3
"""Check native build prerequisites before Cargo compiles dependencies."""
import re
import shutil
import subprocess
import sys

MINIMUMS = {"rustc": (1, 93), "gtk4": (4, 18), "libadwaita-1": (1, 7)}


def version(text):
    match = re.search(r"(\d+)\.(\d+)(?:\.(\d+))?", text)
    return tuple(int(part or 0) for part in match.groups()) if match else ()


def main():
    problems = []
    for tool in ("cargo", "rustc", "pkg-config", "cc"):
        if not shutil.which(tool):
            problems.append(f"Missing {tool}")
    for component, minimum in MINIMUMS.items():
        command = ["rustc", "--version"] if component == "rustc" else ["pkg-config", "--modversion", component]
        try:
            output = subprocess.check_output(command, text=True, stderr=subprocess.STDOUT).strip()
            if version(output) < minimum:
                problems.append(f"{component}: found {output}; need {'.'.join(map(str, minimum))} or newer")
        except (OSError, subprocess.CalledProcessError):
            problems.append(f"Cannot determine {component} version")
    if problems:
        print("Cannot build Spotlight Linux:\n- " + "\n- ".join(problems), file=sys.stderr)
        print("Ubuntu 26.04 is the tested target. Ubuntu 24.04's standard GTK is too old. See README.md.", file=sys.stderr)
        return 1
    print("Build requirements satisfied: Rust >=1.93, GTK >=4.18, libadwaita >=1.7.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
