//! Fixture paths for the preview loop: where the captured view JSON and the
//! rendered HTML/PNG land.

use std::path::Path;
use std::path::PathBuf;

/// The paths a preview run produces, all beside each other under
/// `.scratch/preview/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixturePaths {
    /// The directory holding every fixture file.
    pub dir: PathBuf,
    /// The captured view JSON (the exact payload the render pipeline read).
    pub json: PathBuf,
    /// The rendered HTML page.
    pub html: PathBuf,
    /// The rendered PNG (only written with `--png` and the `png` feature).
    pub png: PathBuf,
}

/// Computes the fixture paths for one plugin command:
/// `<root>/.scratch/preview/<plugin>-<command>.{json,html,png}`, with the
/// plugin and command names sanitized so they can never escape the
/// directory or collide with `.`/`..` (see [`sanitize`]).
pub fn fixture_paths(root: &Path, plugin: &str, command: &str) -> FixturePaths {
    let stem = format!("{}-{}", sanitize(plugin), sanitize(command));
    let dir = root.join(".scratch").join("preview");
    FixturePaths {
        json: dir.join(format!("{stem}.json")),
        html: dir.join(format!("{stem}.html")),
        png: dir.join(format!("{stem}.png")),
        dir,
    }
}

/// Reduces a plugin or command name to a safe filename fragment: `ASCII
/// alphanumerics`, `.`, `_`, and `-` pass through; everything else — path
/// separators included — becomes `_`. An empty or all-dots result becomes
/// `_` so no fixture can turn into a hidden file.
pub fn sanitize(component: &str) -> String {
    let sanitized: String = component
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() || sanitized.chars().all(|c| c == '.') {
        "_".to_owned()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths_for(plugin: &str, command: &str) -> FixturePaths {
        fixture_paths(Path::new("/repo"), plugin, command)
    }

    #[test]
    fn fixture_paths_place_all_outputs_beside_each_other() {
        let paths = paths_for("hello", "hello");
        assert_eq!(paths.dir, Path::new("/repo/.scratch/preview"));
        assert_eq!(
            paths.json,
            Path::new("/repo/.scratch/preview/hello-hello.json")
        );
        assert_eq!(
            paths.html,
            Path::new("/repo/.scratch/preview/hello-hello.html")
        );
        assert_eq!(
            paths.png,
            Path::new("/repo/.scratch/preview/hello-hello.png")
        );
    }

    #[test]
    fn fixture_paths_distinguish_plugin_and_command() {
        let hello = paths_for("hello", "hello");
        let settings = paths_for("settings", "settings");
        assert_ne!(hello.json, settings.json);
        let other_command = paths_for("hello", "hello.tick");
        assert_eq!(
            other_command.json,
            Path::new("/repo/.scratch/preview/hello-hello.tick.json")
        );
    }

    #[test]
    fn sanitize_keeps_filename_safe_characters() {
        assert_eq!(sanitize("hello"), "hello");
        assert_eq!(sanitize("feed.list-2"), "feed.list-2");
        assert_eq!(sanitize("a_b.c-d"), "a_b.c-d");
    }

    #[test]
    fn sanitize_replaces_separators_and_unsafe_characters() {
        assert_eq!(sanitize("a/b"), "a_b");
        assert_eq!(sanitize("..\\.."), ".._..");
        assert_eq!(sanitize("héllo wörld"), "h_llo_w_rld");
        assert_eq!(sanitize("x y"), "x_y");
    }

    #[test]
    fn sanitize_maps_empty_and_dot_only_names_to_underscore() {
        assert_eq!(sanitize(""), "_");
        assert_eq!(sanitize("."), "_");
        assert_eq!(sanitize(".."), "_");
        // Dots inside a longer fragment are fine: the fragment is embedded
        // in a `<plugin>-<command>` stem, never a path component.
        assert_eq!(sanitize(".-."), ".-.");
    }
}
