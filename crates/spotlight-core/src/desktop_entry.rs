use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    io::{self, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{Action, Icon, model::SecondaryAction};

const MAX_DESKTOP_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DesktopApplication {
    pub desktop_id: String,
    pub source_path: PathBuf,
    pub name: String,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub keywords: Vec<String>,
    pub categories: Vec<String>,
    /// Search-only executable basename. This value is never executed.
    pub executable_name: Option<String>,
    pub icon: Icon,
    pub secondary_actions: Vec<SecondaryAction>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogDiagnostics {
    pub files_seen: usize,
    pub applications_indexed: usize,
    pub hidden_or_filtered: usize,
    pub duplicate_ids: usize,
    pub invalid_entries: usize,
    pub unreadable_entries: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DesktopCatalog {
    pub applications: Vec<DesktopApplication>,
    pub diagnostics: CatalogDiagnostics,
}

#[derive(Debug, Error)]
pub enum DesktopEntryError {
    #[error("desktop entry is larger than the {MAX_DESKTOP_FILE_BYTES} byte safety limit")]
    TooLarge,
    #[error("desktop entry has no [Desktop Entry] group")]
    MissingGroup,
    #[error("desktop entry is not an Application")]
    NotApplication,
    #[error("desktop entry has no usable Name")]
    MissingName,
    #[error("failed to read desktop entry: {0}")]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug)]
pub struct DesktopEnvironment {
    pub data_dirs: Vec<PathBuf>,
    pub current_desktops: Vec<String>,
    pub locale_candidates: Vec<String>,
    pub path_dirs: Vec<PathBuf>,
}

impl DesktopEnvironment {
    pub fn from_process() -> Self {
        let home = env::var_os("HOME").map(PathBuf::from);
        let data_home = env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|home| home.join(".local/share")));

        let mut data_dirs = Vec::new();
        if let Some(data_home) = data_home {
            data_dirs.push(data_home.join("applications"));
        }
        data_dirs.extend(
            env::var_os("XDG_DATA_DIRS")
                .filter(|value| !value.is_empty())
                .map(|value| env::split_paths(&value).collect::<Vec<_>>())
                .unwrap_or_else(|| {
                    vec![
                        PathBuf::from("/usr/local/share"),
                        PathBuf::from("/usr/share"),
                    ]
                })
                .into_iter()
                .map(|path| path.join("applications")),
        );

        let current_desktops = env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
            .collect();

        let locale = env::var("LC_ALL")
            .ok()
            .filter(|value| !value.is_empty())
            .or_else(|| {
                env::var("LC_MESSAGES")
                    .ok()
                    .filter(|value| !value.is_empty())
            })
            .or_else(|| env::var("LANG").ok().filter(|value| !value.is_empty()));

        let locale_candidates = locale.as_deref().map(locale_fallbacks).unwrap_or_default();
        let path_dirs = env::var_os("PATH")
            .map(|value| env::split_paths(&value).collect())
            .unwrap_or_default();

        Self {
            data_dirs,
            current_desktops,
            locale_candidates,
            path_dirs,
        }
    }
}

pub fn load_desktop_catalog(environment: &DesktopEnvironment) -> DesktopCatalog {
    let mut catalog = DesktopCatalog::default();
    let mut seen_ids = HashSet::new();

    for root in &environment.data_dirs {
        let mut files = Vec::new();
        collect_desktop_files(root, root, &mut files);
        files.sort();

        for path in files {
            catalog.diagnostics.files_seen += 1;
            let Some(desktop_id) = desktop_id(root, &path) else {
                catalog.diagnostics.invalid_entries += 1;
                continue;
            };

            if seen_ids.contains(&desktop_id) {
                catalog.diagnostics.duplicate_ids += 1;
                continue;
            }

            match read_groups(&path) {
                Ok(groups) => {
                    // Any readable higher-priority file masks a lower-priority ID,
                    // including Hidden=true tombstones.
                    seen_ids.insert(desktop_id.clone());
                    match application_from_groups(desktop_id, path, &groups, environment) {
                        Ok(Some(application)) => catalog.applications.push(application),
                        Ok(None) => catalog.diagnostics.hidden_or_filtered += 1,
                        Err(_) => catalog.diagnostics.invalid_entries += 1,
                    }
                }
                Err(DesktopEntryError::Io(_)) => catalog.diagnostics.unreadable_entries += 1,
                Err(_) => catalog.diagnostics.invalid_entries += 1,
            }
        }
    }

    catalog
        .applications
        .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    catalog.diagnostics.applications_indexed = catalog.applications.len();
    catalog
}

fn application_from_groups(
    desktop_id: String,
    source_path: PathBuf,
    groups: &Groups,
    environment: &DesktopEnvironment,
) -> Result<Option<DesktopApplication>, DesktopEntryError> {
    let entry = groups
        .get("Desktop Entry")
        .ok_or(DesktopEntryError::MissingGroup)?;

    if entry.get("Type").map(String::as_str) != Some("Application") {
        return Err(DesktopEntryError::NotApplication);
    }
    if bool_value(entry.get("Hidden")) || bool_value(entry.get("NoDisplay")) {
        return Ok(None);
    }
    if !desktop_visible(entry, &environment.current_desktops) {
        return Ok(None);
    }
    if let Some(try_exec) = entry.get("TryExec")
        && !executable_available(&unescape_value(try_exec), &environment.path_dirs)
    {
        return Ok(None);
    }

    let name = localized_value(entry, "Name", &environment.locale_candidates)
        .filter(|name| !name.trim().is_empty())
        .ok_or(DesktopEntryError::MissingName)?;
    let generic_name = localized_value(entry, "GenericName", &environment.locale_candidates);
    let comment = localized_value(entry, "Comment", &environment.locale_candidates);
    let keywords = localized_value(entry, "Keywords", &environment.locale_candidates)
        .map(|value| parse_list(&value))
        .unwrap_or_default();
    let categories = entry
        .get("Categories")
        .map(|value| parse_list(value))
        .unwrap_or_default();
    let executable_name = entry
        .get("Exec")
        .and_then(|value| extract_executable_name(value));
    let icon = entry
        .get("Icon")
        .filter(|value| !value.trim().is_empty())
        .map(|value| {
            let value = unescape_value(value);
            if Path::new(&value).is_absolute() {
                Icon::File(value)
            } else {
                Icon::Themed(value)
            }
        })
        .unwrap_or_default();

    let secondary_actions = entry
        .get("Actions")
        .map(|value| parse_list(value))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|action_id| {
            let group = groups.get(&format!("Desktop Action {action_id}"))?;
            let title = localized_value(group, "Name", &environment.locale_candidates)?;
            let action_icon = group
                .get("Icon")
                .map(|value| Icon::Themed(unescape_value(value)))
                .unwrap_or_else(|| Icon::Themed("system-run-symbolic".into()));
            Some(SecondaryAction {
                title,
                icon: action_icon,
                shortcut_hint: None,
                action: Action::LaunchDesktopAction {
                    desktop_id: desktop_id.clone(),
                    action_id,
                },
            })
        })
        .collect();

    Ok(Some(DesktopApplication {
        desktop_id,
        source_path,
        name,
        generic_name,
        comment,
        keywords,
        categories,
        executable_name,
        icon,
        secondary_actions,
    }))
}

type Group = BTreeMap<String, String>;
type Groups = BTreeMap<String, Group>;

fn read_groups(path: &Path) -> Result<Groups, DesktopEntryError> {
    let file = fs::File::open(path)?;
    let mut contents = String::new();
    file.take(MAX_DESKTOP_FILE_BYTES + 1)
        .read_to_string(&mut contents)?;
    if contents.len() as u64 > MAX_DESKTOP_FILE_BYTES {
        return Err(DesktopEntryError::TooLarge);
    }
    Ok(parse_groups(&contents))
}

fn parse_groups(contents: &str) -> Groups {
    let mut groups = Groups::new();
    let mut current_group: Option<String> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
            let name = line[1..line.len() - 1].trim();
            if !name.is_empty() {
                current_group = Some(name.to_owned());
                groups.entry(name.to_owned()).or_default();
            }
            continue;
        }
        let Some(group_name) = current_group.as_ref() else {
            continue;
        };
        let Some((key, value)) = raw_line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            continue;
        }
        groups
            .entry(group_name.clone())
            .or_default()
            .insert(key.to_owned(), value.trim().to_owned());
    }
    groups
}

fn localized_value(group: &Group, key: &str, locale_candidates: &[String]) -> Option<String> {
    locale_candidates
        .iter()
        .find_map(|locale| group.get(&format!("{key}[{locale}]")))
        .or_else(|| group.get(key))
        .map(|value| unescape_value(value))
}

pub fn locale_fallbacks(locale: &str) -> Vec<String> {
    let (locale_without_modifier, modifier) = locale
        .split_once('@')
        .map_or((locale, None), |(base, modifier)| (base, Some(modifier)));
    let base = locale_without_modifier
        .split('.')
        .next()
        .unwrap_or(locale_without_modifier);
    if base.eq_ignore_ascii_case("c") || base.eq_ignore_ascii_case("posix") {
        return vec![];
    }
    let (language, territory) = base
        .split_once('_')
        .map_or((base, None), |(language, territory)| {
            (language, Some(territory))
        });

    let mut values = Vec::new();
    if let (Some(territory), Some(modifier)) = (territory, modifier) {
        values.push(format!("{language}_{territory}@{modifier}"));
    }
    if let Some(territory) = territory {
        values.push(format!("{language}_{territory}"));
    }
    if let Some(modifier) = modifier {
        values.push(format!("{language}@{modifier}"));
    }
    values.push(language.to_owned());
    values.dedup();
    values
}

fn desktop_visible(group: &Group, current_desktops: &[String]) -> bool {
    let only = group
        .get("OnlyShowIn")
        .map(|value| parse_list(value))
        .unwrap_or_default();
    if !only.is_empty()
        && !only.iter().any(|desktop| {
            current_desktops
                .iter()
                .any(|current| current.eq_ignore_ascii_case(desktop))
        })
    {
        return false;
    }

    let excluded = group
        .get("NotShowIn")
        .map(|value| parse_list(value))
        .unwrap_or_default();
    !excluded.iter().any(|desktop| {
        current_desktops
            .iter()
            .any(|current| current.eq_ignore_ascii_case(desktop))
    })
}

fn collect_desktop_files(root: &Path, directory: &Path, output: &mut Vec<PathBuf>) {
    let mut pending = vec![directory.to_owned()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && path.extension().is_some_and(|value| value == "desktop")
            {
                output.push(path);
            }
        }

        // A malformed root should never accidentally scan above itself.
        debug_assert!(directory.starts_with(root));
    }
}

fn desktop_id(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    let last = parts.last_mut()?;
    *last = last.strip_suffix(".desktop")?;
    Some(format!("{}.desktop", parts.join("-")))
}

fn bool_value(value: Option<&String>) -> bool {
    value.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn parse_list(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for character in value.chars() {
        if escaped {
            current.push(unescaped_character(character));
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ';' {
            if !current.is_empty() {
                values.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        values.push(current);
    }
    values
}

fn unescape_value(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut escaped = false;
    for character in value.chars() {
        if escaped {
            output.push(unescaped_character(character));
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    if escaped {
        output.push('\\');
    }
    output
}

fn unescaped_character(character: char) -> char {
    match character {
        's' => ' ',
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        other => other,
    }
}

/// Parses only the first token for search metadata. It does not perform expansion,
/// invoke a shell, or produce launch arguments.
pub fn extract_executable_name(exec: &str) -> Option<String> {
    let mut token = String::new();
    let mut quoted = false;
    let mut escaped = false;

    for character in exec.trim_start().chars() {
        if escaped {
            token.push(character);
            escaped = false;
        } else {
            match character {
                '\\' => escaped = true,
                '"' => quoted = !quoted,
                value if value.is_whitespace() && !quoted => break,
                value => token.push(value),
            }
        }
    }
    if escaped || quoted || token.is_empty() || token.contains('%') {
        return None;
    }
    Path::new(&token)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn executable_available(value: &str, path_dirs: &[PathBuf]) -> bool {
    if value.is_empty() {
        return false;
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return is_executable_file(path);
    }
    path_dirs
        .iter()
        .any(|directory| is_executable_file(&directory.join(path)))
}

fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use tempfile::tempdir;

    use super::*;

    fn environment(paths: Vec<PathBuf>) -> DesktopEnvironment {
        DesktopEnvironment {
            data_dirs: paths,
            current_desktops: vec!["gnome".into()],
            locale_candidates: locale_fallbacks("fr_CA.UTF-8"),
            path_dirs: vec![],
        }
    }

    #[test]
    fn parses_localized_search_fields_and_actions() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("org.example.Editor.desktop");
        fs::write(
            &path,
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Editor\n\
             Name[fr]=Éditeur\n\
             GenericName=Text Editor\n\
             Keywords=write;notes;\n\
             Categories=Utility;TextEditor;\n\
             Exec=/usr/bin/editor --new-window %U\n\
             Icon=org.example.Editor\n\
             Actions=private;\n\
             [Desktop Action private]\n\
             Name=New Private Window\n\
             Exec=/usr/bin/editor --private\n",
        )
        .unwrap();

        let catalog = load_desktop_catalog(&environment(vec![directory.path().into()]));
        assert_eq!(catalog.applications.len(), 1);
        let application = &catalog.applications[0];
        assert_eq!(application.desktop_id, "org.example.Editor.desktop");
        assert_eq!(application.name, "Éditeur");
        assert_eq!(application.executable_name.as_deref(), Some("editor"));
        assert_eq!(application.keywords, ["write", "notes"]);
        assert_eq!(application.secondary_actions.len(), 1);
    }

    #[test]
    fn high_priority_hidden_entry_masks_system_entry() {
        let user = tempdir().unwrap();
        let system = tempdir().unwrap();
        fs::write(
            user.path().join("masked.desktop"),
            "[Desktop Entry]\nType=Application\nName=Masked\nHidden=true\n",
        )
        .unwrap();
        fs::write(
            system.path().join("masked.desktop"),
            "[Desktop Entry]\nType=Application\nName=Should not appear\n",
        )
        .unwrap();

        let catalog =
            load_desktop_catalog(&environment(vec![user.path().into(), system.path().into()]));
        assert!(catalog.applications.is_empty());
        assert_eq!(catalog.diagnostics.duplicate_ids, 1);
    }

    #[test]
    fn desktop_visibility_is_honored() {
        let directory = tempdir().unwrap();
        fs::write(
            directory.path().join("kde.desktop"),
            "[Desktop Entry]\nType=Application\nName=KDE only\nOnlyShowIn=KDE;\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("not-gnome.desktop"),
            "[Desktop Entry]\nType=Application\nName=Not GNOME\nNotShowIn=GNOME;\n",
        )
        .unwrap();
        assert!(
            load_desktop_catalog(&environment(vec![directory.path().into()]))
                .applications
                .is_empty()
        );
    }

    #[test]
    fn exec_parser_never_interprets_shell_syntax() {
        assert_eq!(
            extract_executable_name("/usr/bin/notify-send '$(touch /tmp/bad)'"),
            Some("notify-send".into())
        );
        assert_eq!(
            extract_executable_name("\"/opt/My App/editor\" --open %U"),
            Some("editor".into())
        );
        assert_eq!(extract_executable_name("%u --invalid"), None);
        assert_eq!(extract_executable_name("\"unterminated"), None);
    }

    #[test]
    fn try_exec_requires_an_executable_regular_file() {
        let directory = tempdir().unwrap();
        let candidate = directory.path().join("candidate");
        fs::write(&candidate, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o600)).unwrap();

        assert!(!executable_available(
            "candidate",
            &[directory.path().into()]
        ));
        fs::set_permissions(&candidate, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(executable_available(
            "candidate",
            &[directory.path().into()]
        ));
    }

    #[test]
    fn desktop_file_reads_are_bounded_even_if_metadata_changes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("oversized.desktop");
        fs::write(&path, vec![b'a'; MAX_DESKTOP_FILE_BYTES as usize + 1]).unwrap();

        assert!(matches!(
            read_groups(&path),
            Err(DesktopEntryError::TooLarge)
        ));
    }

    #[test]
    fn locale_candidates_follow_specific_to_general_order() {
        assert_eq!(
            locale_fallbacks("sr_RS.UTF-8@latin"),
            ["sr_RS@latin", "sr_RS", "sr@latin", "sr"]
        );
    }
}
