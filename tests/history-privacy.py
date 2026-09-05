#!/usr/bin/env python3
"""Exercise the real UsageStore under umask 022, including a second UID in CI."""
import os
from pathlib import Path
import subprocess
import tempfile

repo = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="spotlight-privacy-") as directory:
    root = Path(directory)
    root.chmod(0o755)
    project = root / "probe"
    (project / "src").mkdir(parents=True)
    (project / "Cargo.toml").write_text('[package]\nname="spotlight-privacy-probe"\nversion="0.0.0"\nedition="2024"\n[dependencies]\nspotlight-core={path=' + '"' + str(repo / "crates/spotlight-core") + '"}\n')
    (project / "src/main.rs").write_text('''use spotlight_core::history::UsageStore;
use std::io::{self, Write};
fn main() {
 let path=std::env::args().nth(1).unwrap();
 let mut store=UsageStore::open(&path).unwrap();
 store.record_launch_at("application:synthetic.desktop",1700000000).unwrap();
 println!("ready");io::stdout().flush().unwrap();
 let mut line=String::new();io::stdin().read_line(&mut line).unwrap();
}
''')
    subprocess.run(["cargo", "build", "--offline", "--manifest-path", str(project / "Cargo.toml"), "--target-dir", str(repo / "target/privacy-probe")], check=True)
    path = root / "data" / "spotlight-linux" / "history.sqlite3"
    # The parent is deliberately traversable; only the application-owned leaf is private.
    path.parent.parent.mkdir()
    path.parent.parent.chmod(0o755)
    proc = subprocess.Popen([str(repo / "target/privacy-probe/debug/spotlight-privacy-probe"), str(path)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, umask=0o022)
    try:
        assert proc.stdout.readline().strip() == "ready"
        assert path.parent.stat().st_mode & 0o777 == 0o700
        assert path.parent.parent.stat().st_mode & 0o777 == 0o755
        files = [Path(str(path) + suffix) for suffix in ("", "-wal", "-shm")]
        for file in files:
            assert file.stat().st_mode & 0o777 == 0o600, file
        if os.geteuid() == 0:
            def other_user():
                os.setgroups([])
                os.setgid(65534)
                os.setuid(65534)
            for file in files:
                result = subprocess.run(["python3", "-c", "from pathlib import Path; import sys; Path(sys.argv[1]).read_bytes()", str(file)], preexec_fn=other_user, capture_output=True, text=True)
                assert result.returncode != 0 and "PermissionError" in result.stderr, result.stderr
            print("PASS: another local UID cannot read database/WAL/SHM")
        else:
            print("PASS: 0700/0600 under umask 022; second-UID check requires root and runs in the CI container")
    finally:
        proc.communicate("done\n", timeout=10)
        assert proc.returncode == 0
