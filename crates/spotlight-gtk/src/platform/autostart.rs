use std::{fs, io, path::Path};

use spotlight_core::settings::APPLICATION_ID;

const MARKER: &str = "X-SpotlightLinux-Managed=true";

pub fn set_enabled(path: &Path, enabled: bool) -> io::Result<()> {
    set_enabled_for(path, &std::env::current_exe()?, enabled)
}

pub fn set_enabled_for(path: &Path, executable: &Path, enabled: bool) -> io::Result<()> {
    if path.exists() && !is_managed(path) {
        return Err(io::Error::other(
            "This autostart entry is not owned by Spotlight Linux; it has been preserved",
        ));
    }
    if !enabled {
        return match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        };
    }
    let contents = format!(
        "[Desktop Entry]\nType=Application\nName=Spotlight Linux\nComment=Keep the launcher shortcut ready\nExec={} --background\nIcon={APPLICATION_ID}\nTerminal=false\nNoDisplay=true\nX-GNOME-Autostart-enabled=true\n{MARKER}\n",
        quote_exec_argument(executable)?
    );
    super::installation::atomic_write(path, contents.as_bytes(), 0o600)
}

pub fn is_managed(path: &Path) -> bool {
    fs::read_to_string(path).is_ok_and(|contents| contents.lines().any(|line| line == MARKER))
}

/// Desktop entry escaping has two layers: key-file escapes, then Exec arguments.
/// This is not a shell command. Literal percent signs must escape field codes.
pub(super) fn quote_exec_argument(path: &Path) -> io::Result<String> {
    let value = path
        .to_str()
        .filter(|value| !value.contains(['\n', '\r', '\0']))
        .ok_or_else(|| {
            io::Error::other("Executable path cannot be represented in desktop metadata")
        })?;
    let mut argument = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' | '"' | '`' | '$' => {
                argument.push('\\');
                argument.push(ch);
            }
            '%' => argument.push_str("%%"),
            _ => argument.push(ch),
        }
    }
    argument.push('"');
    Ok(argument.replace('\\', "\\\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_escaping_is_not_shell_interpolation() {
        assert_eq!(
            quote_exec_argument(Path::new("/tmp/My $App/100%")).unwrap(),
            "\"/tmp/My \\\\$App/100%%\""
        );
        assert!(quote_exec_argument(Path::new("/tmp/injected\nExec=bad")).is_err());
    }

    #[test]
    fn refuses_to_remove_or_overwrite_unowned_autostart() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("custom.desktop");
        fs::write(&path, b"user-owned").unwrap();
        assert!(set_enabled_for(&path, Path::new("/bin/true"), false).is_err());
        assert!(set_enabled_for(&path, Path::new("/bin/true"), true).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"user-owned");
    }
}
