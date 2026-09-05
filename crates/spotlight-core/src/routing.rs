//! Pure query-mode routing.
//!
//! Keeping routing separate from execution makes it possible to prove that a
//! normal search can never become a shell command accidentally. This module does
//! not execute commands; it only identifies an explicitly requested mode.

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueryRoute {
    Unified { query: String },
    Shell { command: String },
}

/// Route a raw query according to enabled capabilities.
///
/// Shell mode requires both the setting and `>` as the very first character.
/// Leading whitespace intentionally prevents the mode switch.
pub fn route_query(raw: &str, shell_enabled: bool) -> QueryRoute {
    if shell_enabled && let Some(command) = raw.strip_prefix('>') {
        return QueryRoute::Shell {
            command: command.trim_start().to_owned(),
        };
    }

    QueryRoute::Unified {
        query: raw.trim().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_is_always_a_unified_search() {
        assert_eq!(
            route_query("rm -rf Documents", true),
            QueryRoute::Unified {
                query: "rm -rf Documents".into(),
            }
        );
    }

    #[test]
    fn shell_mode_requires_an_explicit_first_character_prefix() {
        assert_eq!(
            route_query("> printf hello", true),
            QueryRoute::Shell {
                command: "printf hello".into(),
            }
        );
        assert!(matches!(
            route_query(" > printf hello", true),
            QueryRoute::Unified { .. }
        ));
    }

    #[test]
    fn disabling_shell_mode_makes_the_prefix_plain_search_text() {
        assert_eq!(
            route_query("> shutdown now", false),
            QueryRoute::Unified {
                query: "> shutdown now".into(),
            }
        );
    }
}
