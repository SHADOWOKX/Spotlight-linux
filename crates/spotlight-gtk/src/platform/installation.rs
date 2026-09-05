//! User-local installation. Never depends on the desktop session's PATH.
use std::{
    collections::BTreeMap,
    env, fs, io,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use gtk::{gio, glib, prelude::*};
use serde::{Deserialize, Serialize};
use spotlight_core::settings::{APPLICATION_ID, SettingsStore, XdgPaths};

use super::autostart;

const DESKTOP: &str = include_str!("../../../../data/io.github.shadowokx.SpotlightLinux.desktop");
const SERVICE: &str = include_str!("../../../../data/io.github.shadowokx.SpotlightLinux.service");
const ICON: &str = include_str!("../../../../data/io.github.shadowokx.SpotlightLinux.svg");
const METAINFO: &str =
    include_str!("../../../../data/io.github.shadowokx.SpotlightLinux.metainfo.xml");

#[derive(Clone)]
pub struct InstallPaths {
    pub binary: PathBuf,
    pub data: PathBuf,
    pub xdg: XdgPaths,
}

#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    files: BTreeMap<PathBuf, String>,
}

impl InstallPaths {
    pub fn from_process() -> io::Result<Self> {
        let xdg = XdgPaths::from_process().map_err(io::Error::other)?;
        let data = xdg
            .data_dir
            .parent()
            .ok_or_else(|| io::Error::other("Invalid XDG data directory"))?
            .to_owned();
        let bin = env::var_os("XDG_BIN_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin")))
            .ok_or_else(|| io::Error::other("HOME is not configured"))?;
        if !bin.is_absolute() || !data.is_absolute() || !xdg.config_dir.is_absolute() {
            return Err(io::Error::other(
                "XDG installation directories must be absolute paths",
            ));
        }
        Ok(Self {
            binary: bin.join("spotlight-linux"),
            data,
            xdg,
        })
    }

    pub fn desktop(&self) -> PathBuf {
        self.data
            .join("applications")
            .join(format!("{APPLICATION_ID}.desktop"))
    }

    fn manifest(&self) -> PathBuf {
        self.xdg.data_dir.join("install-manifest.toml")
    }

    fn assets(&self, executable: &[u8]) -> io::Result<Vec<(PathBuf, Vec<u8>, u32)>> {
        // Gio validates Exec before expanding %% field codes, so a literal %
        // in the binary path cannot round-trip through its desktop loader.
        if self
            .binary
            .to_str()
            .is_none_or(|value| value.contains('%') || value.chars().any(char::is_control))
        {
            return Err(io::Error::other(
                "Choose an installation path without percent signs, control characters, or non-UTF-8 bytes. Spaces are supported.",
            ));
        }
        let exec = autostart::quote_exec_argument(&self.binary)?;
        let desktop = DESKTOP.replace("Exec=spotlight-linux\n", &format!("Exec={exec}\n"));
        // D-Bus service Exec uses shell tokenization without shell expansion or
        // desktop field codes. Quote it separately from Desktop Entry Exec.
        let service = SERVICE.replace(
            "Exec=spotlight-linux --gapplication-service",
            &format!(
                "Exec={} --gapplication-service",
                service_exec(&self.binary)?
            ),
        );
        Ok(vec![
            (self.binary.clone(), executable.to_vec(), 0o755),
            (self.desktop(), desktop.into_bytes(), 0o644),
            (
                self.data
                    .join("dbus-1/services")
                    .join(format!("{APPLICATION_ID}.service")),
                service.into_bytes(),
                0o644,
            ),
            (
                self.data
                    .join("icons/hicolor/scalable/apps")
                    .join(format!("{APPLICATION_ID}.svg")),
                ICON.as_bytes().to_vec(),
                0o644,
            ),
            (
                self.data
                    .join("metainfo")
                    .join(format!("{APPLICATION_ID}.metainfo.xml")),
                METAINFO.as_bytes().to_vec(),
                0o644,
            ),
        ])
    }
}

pub fn install(paths: &InstallPaths, executable: &Path) -> io::Result<Vec<PathBuf>> {
    let bytes = fs::read(executable)?;
    let assets = paths.assets(&bytes)?;
    let mut manifest = Manifest::default();
    for (path, contents, mode) in assets {
        atomic_write(&path, &contents, mode)?;
        manifest.files.insert(path, checksum(&contents));
    }
    if SettingsStore::new(paths.xdg.settings_file())
        .load()
        .map_err(io::Error::other)?
        .general
        .launch_at_login
    {
        autostart::set_enabled_for(&paths.xdg.autostart_file, &paths.binary, true)
            .map_err(io::Error::other)?;
    }
    atomic_write(
        &paths.manifest(),
        toml::to_string_pretty(&manifest)
            .map_err(io::Error::other)?
            .as_bytes(),
        0o600,
    )?;
    Ok(manifest.files.into_keys().collect())
}

/// Maintenance only: stop our exact executable before replacement. A pidfd
/// prevents PID-reuse races. Prefer Quit; the legacy release needs SIGTERM.
pub fn stop_running(paths: &InstallPaths) -> io::Result<()> {
    let Ok(bus) = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>) else {
        return Ok(());
    };
    let owner = bus.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "GetNameOwner",
        Some(&(APPLICATION_ID,).to_variant()),
        None,
        gio::DBusCallFlags::NO_AUTO_START,
        3000,
        None::<&gio::Cancellable>,
    );
    let owner = match owner {
        Ok(value) => {
            value
                .get::<(String,)>()
                .ok_or_else(|| io::Error::other("Invalid D-Bus owner response"))?
                .0
        }
        Err(error) if error.matches(gio::DBusError::NameHasNoOwner) => return Ok(()),
        Err(error) => return Err(io::Error::other(error)),
    };
    let response = bus
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "GetConnectionUnixProcessID",
            Some(&(&owner,).to_variant()),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            3000,
            None::<&gio::Cancellable>,
        )
        .map_err(io::Error::other)?;
    let pid = response
        .get::<(u32,)>()
        .ok_or_else(|| io::Error::other("Invalid D-Bus PID response"))?
        .0;
    let pid_number = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .ok_or_else(|| io::Error::other("Invalid resident PID"))?;
    let process = rustix::process::pidfd_open(pid_number, rustix::process::PidfdFlags::empty())?;
    let executable = fs::read_link(format!("/proc/{pid}/exe"))?;
    let expected = paths
        .binary
        .canonicalize()
        .unwrap_or_else(|_| paths.binary.clone());
    let own = std::env::current_exe()?;
    let actual = executable.to_string_lossy();
    let actual = Path::new(actual.strip_suffix(" (deleted)").unwrap_or(&actual));
    if actual != expected && actual != own {
        return Err(io::Error::other(format!(
            "The application ID is owned by a different executable ({}). Close that instance before installation; it was not terminated.",
            executable.display()
        )));
    }
    let actions = bus
        .call_sync(
            Some(&owner),
            "/io/github/shadowokx/SpotlightLinux",
            "org.gtk.Actions",
            "List",
            None,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            3000,
            None::<&gio::Cancellable>,
        )
        .ok()
        .and_then(|v| v.get::<(Vec<String>,)>());
    if actions.is_some_and(|(names,)| names.iter().any(|name| name == "quit")) {
        let parameters = (
            "quit",
            Vec::<glib::Variant>::new(),
            std::collections::HashMap::<String, glib::Variant>::new(),
        )
            .to_variant();
        bus.call_sync(
            Some(&owner),
            "/io/github/shadowokx/SpotlightLinux",
            "org.gtk.Actions",
            "Activate",
            Some(&parameters),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            3000,
            None::<&gio::Cancellable>,
        )
        .map_err(io::Error::other)?;
    } else {
        eprintln!("Stopping legacy Spotlight Linux (PID {pid}) for desktop-integration upgrade");
        rustix::process::pidfd_send_signal(&process, rustix::process::Signal::TERM)?;
    }
    let mut events = [rustix::event::PollFd::new(
        &process,
        rustix::event::PollFlags::IN,
    )];
    if rustix::event::poll(
        &mut events,
        Some(&rustix::event::Timespec {
            tv_sec: 5,
            tv_nsec: 0,
        }),
    )? == 0
    {
        return Err(io::Error::other(
            "Spotlight Linux did not exit within five seconds; close it before continuing. No force-kill was attempted.",
        ));
    }
    Ok(())
}

/// Preserve modified files, all personal data, and every unrecognized path.
pub fn uninstall(paths: &InstallPaths) -> io::Result<Vec<PathBuf>> {
    let manifest_text = fs::read_to_string(paths.manifest())?;
    let mut manifest: Manifest = toml::from_str(&manifest_text).map_err(io::Error::other)?;
    let allowed = paths
        .assets(&[])?
        .into_iter()
        .map(|(path, _, _)| path)
        .collect::<Vec<_>>();
    let mut removed = Vec::new();
    for path in allowed {
        let Some(expected) = manifest.files.get(&path) else {
            continue;
        };
        match fs::read(&path) {
            Ok(contents) if checksum(&contents) == *expected => {
                fs::remove_file(&path)?;
                manifest.files.remove(&path);
                removed.push(path);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                manifest.files.remove(&path);
            }
            Ok(_) => eprintln!("Preserved modified file: {}", path.display()),
            Err(error) => return Err(error),
        }
    }
    // Autostart is created/changed by Settings, so use its ownership marker.
    if autostart::is_managed(&paths.xdg.autostart_file) {
        fs::remove_file(&paths.xdg.autostart_file)?;
        removed.push(paths.xdg.autostart_file.clone());
    }
    if manifest.files.is_empty() {
        fs::remove_file(paths.manifest())?;
        removed.push(paths.manifest());
    } else {
        atomic_write(
            &paths.manifest(),
            toml::to_string_pretty(&manifest)
                .map_err(io::Error::other)?
                .as_bytes(),
            0o600,
        )?;
    }
    Ok(removed)
}

pub(super) fn atomic_write(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing destination directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    fs::File::open(parent)?.sync_all()
}

fn checksum(contents: &[u8]) -> String {
    let mut checksum =
        glib::Checksum::new(glib::ChecksumType::Sha256).expect("GLib supports SHA-256");
    checksum.update(contents);
    checksum
        .string()
        .expect("SHA-256 has a string representation")
}

fn service_exec(path: &Path) -> io::Result<String> {
    let value = path
        .to_str()
        .filter(|value| !value.contains(['\n', '\r', '\0']))
        .ok_or_else(|| {
            io::Error::other("Executable path cannot be represented in desktop metadata")
        })?;
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gio_unix::DesktopAppInfo;

    fn layout(root: &Path) -> InstallPaths {
        InstallPaths {
            binary: root.join("bin with spaces/spotlight-linux"),
            data: root.join("data"),
            xdg: XdgPaths {
                config_dir: root.join("config/spotlight-linux"),
                cache_dir: root.join("cache/spotlight-linux"),
                data_dir: root.join("data/spotlight-linux"),
                autostart_file: root
                    .join("config/autostart")
                    .join(format!("{APPLICATION_ID}.desktop")),
            },
        }
    }

    #[test]
    fn installed_identity_resolves_with_absolute_exec_and_uninstall_preserves_user_data() {
        let root = tempfile::tempdir().unwrap();
        let paths = layout(root.path());
        let source = root.path().join("source");
        fs::write(&source, b"test executable").unwrap();
        let installed = install(&paths, &source).unwrap();
        let app = DesktopAppInfo::from_filename(paths.desktop()).unwrap();
        assert_eq!(app.name(), "Spotlight Linux");
        // GAppInfo::executable historically splits on the first space without
        // unquoting. Check the actual Exec argv, not that lossy display helper.
        let argv = glib::shell_parse_argv(app.commandline().unwrap()).unwrap();
        assert_eq!(Path::new(&argv[0]), paths.binary);
        assert!(app.boolean("DBusActivatable"));
        let user_file = paths.xdg.data_dir.join("history.sqlite3");
        fs::write(&user_file, b"keep personal data").unwrap();
        autostart::set_enabled_for(&paths.xdg.autostart_file, &paths.binary, true).unwrap();
        uninstall(&paths).unwrap();
        assert!(installed.iter().all(|file| !file.exists()));
        assert!(!paths.xdg.autostart_file.exists());
        assert!(user_file.exists());
    }

    #[test]
    fn uninstall_does_not_follow_untrusted_manifest_paths_or_delete_modified_assets() {
        let root = tempfile::tempdir().unwrap();
        let paths = layout(root.path());
        let source = root.path().join("unrelated-file");
        fs::write(&source, b"untouched").unwrap();
        install(&paths, &source).unwrap();
        fs::write(paths.desktop(), "user-edited desktop entry").unwrap();
        let mut manifest: Manifest =
            toml::from_str(&fs::read_to_string(paths.manifest()).unwrap()).unwrap();
        manifest
            .files
            .insert(source.clone(), checksum(b"untouched"));
        fs::write(paths.manifest(), toml::to_string(&manifest).unwrap()).unwrap();
        uninstall(&paths).unwrap();
        assert!(source.exists());
        assert!(paths.desktop().exists());
    }
}
