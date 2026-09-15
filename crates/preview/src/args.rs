//! Command-line parsing for the preview tool.

use crate::resolve::PluginSpec;

/// The plugin binary spawned when `--plugin` is absent.
pub const DEFAULT_PLUGIN: &str = "hello";

/// A parsed command line: either a [`PreviewArgs`] to run, or the usage text
/// requested by `--help`/`-h`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    /// Run a preview with these arguments.
    Run(PreviewArgs),
    /// Print this usage text instead of running.
    Help(String),
}
/// The preview run's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewArgs {
    /// Which plugin to spawn: a bare binary name (resolved against the cargo
    /// build profile directories, see [`crate::resolve`]) or an explicit
    /// path. Defaults to [`DEFAULT_PLUGIN`].
    pub plugin: PluginSpec,
    /// The command to invoke. Defaults to the plugin manifest's single
    /// declared command (see [`crate::proto::single_command_name`]).
    pub command: Option<String>,
    /// Also render a PNG of the HTML (requires a build with the `png`
    /// feature; see the crate docs).
    pub png: bool,
}

/// Parses the preview tool's arguments (argv without the program name).
///
/// Flags: `--plugin <name|path>` (also `--plugin=<…>`), `--command <name>`
/// (also `--command=<…>`), `--png`, `-h`/`--help`. A repeated flag wins with
/// its last value. Everything else is an error naming the offending
/// argument.
pub fn parse(argv: &[String]) -> Result<Parsed, String> {
    let mut plugin = None;
    let mut command = None;
    let mut png = false;
    let mut positional_only = false;

    let mut index = 0;
    while index < argv.len() {
        let arg = argv[index].as_str();
        index += 1;

        if positional_only {
            return Err(unexpected_argument(arg));
        }
        if arg == "--" {
            positional_only = true;
            continue;
        }

        let (name, inline) = match arg.split_once('=') {
            Some((name, inline)) => (name, Some(inline.to_owned())),
            None => (arg, None),
        };
        match name {
            "--help" | "-h" => return Ok(Parsed::Help(usage())),
            "--png" => {
                if inline.is_some() {
                    return Err(format!("`{arg}` takes no value"));
                }
                png = true;
            }
            "--plugin" | "--command" => {
                let value = match inline {
                    Some(value) => value,
                    None => match argv.get(index) {
                        Some(value) if !value.starts_with('-') => {
                            index += 1;
                            value.clone()
                        }
                        _ => return Err(format!("`{name}` needs a value")),
                    },
                };
                if value.is_empty() {
                    return Err(format!("`{name}` needs a non-empty value"));
                }
                if name == "--plugin" {
                    plugin = Some(PluginSpec::from_flag(&value));
                } else {
                    command = Some(value);
                }
            }
            other => {
                return Err(if other.starts_with('-') {
                    format!(
                        "unknown flag `{other}`; known flags: --plugin <name|path>, \
                         --command <name>, --png, --help"
                    )
                } else {
                    unexpected_argument(other)
                });
            }
        }
    }

    Ok(Parsed::Run(PreviewArgs {
        plugin: plugin.unwrap_or_else(|| PluginSpec::from_flag(DEFAULT_PLUGIN)),
        command,
        png,
    }))
}

/// The message for an argument that is not a flag this tool takes.
fn unexpected_argument(arg: &str) -> String {
    format!("unexpected argument `{arg}`; this tool takes flags only (see --help)")
}

/// The usage text printed for `--help`.
fn usage() -> String {
    let mut text = String::from(
        "Render a plugin view to a fixture, an HTML page, and an optional PNG.\n\
         \n\
         Usage: pwr-preview [--plugin <name|path>] [--command <name>] [--png]\n",
    );
    text.push_str("\nOptions:\n");
    text.push_str(&format!(
        "  --plugin <name|path>  Plugin binary to spawn (default: {DEFAULT_PLUGIN}). A bare\n\
         \x20                       name resolves next to this executable, then the debug and\n\
         \x20                       release profile dirs of the same target dir.\n"
    ));
    text.push_str(
        "  --command <name>      Command to invoke (default: the manifest's single command).\n\
         \x20 --png                 Also render a PNG (requires a build with the `png` feature).\n\
         \x20 -h, --help            Print this help.\n",
    );
    text.push_str(
        "\nExamples:\n\
         \x20 ./dev.sh preview\n\
         \x20 ./dev.sh preview -- --png\n\
         \x20 ./dev.sh preview -- --plugin settings\n",
    );
    text.push_str("\nFixtures land in ./.scratch/preview/ beside the rendered HTML.\n");
    text
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn run(argv: &[&str]) -> PreviewArgs {
        let argv: Vec<String> = argv.iter().map(|a| (*a).to_owned()).collect();
        match parse(&argv).expect("parse") {
            Parsed::Run(args) => args,
            Parsed::Help(_) => panic!("expected a run, got help"),
        }
    }

    fn err(argv: &[&str]) -> String {
        let argv: Vec<String> = argv.iter().map(|a| (*a).to_owned()).collect();
        match parse(&argv) {
            Err(message) => message,
            Ok(Parsed::Help(text)) => panic!("expected a parse error, got help: {text}"),
            Ok(Parsed::Run(args)) => panic!("expected a parse error, got a run: {args:?}"),
        }
    }

    #[test]
    fn no_arguments_defaults_to_the_hello_plugin() {
        let args = run(&[]);
        assert_eq!(args.plugin, PluginSpec::Name("hello".into()));
        assert_eq!(args.command, None);
        assert!(!args.png);
    }

    #[test]
    fn png_flag_enables_png_rendering() {
        assert!(run(&["--png"]).png);
    }

    #[test]
    fn plugin_flag_takes_the_next_argument_as_a_name() {
        assert_eq!(
            run(&["--plugin", "settings"]).plugin,
            PluginSpec::Name("settings".into())
        );
    }

    #[test]
    fn plugin_flag_accepts_the_equals_form_and_paths() {
        assert_eq!(
            run(&["--plugin=./target/debug/hello"]).plugin,
            PluginSpec::Path(PathBuf::from("./target/debug/hello"))
        );
    }

    #[test]
    fn command_flag_takes_space_and_equals_forms() {
        assert_eq!(
            run(&["--command", "hello"]).command.as_deref(),
            Some("hello")
        );
        assert_eq!(
            run(&["--command=feed.list"]).command.as_deref(),
            Some("feed.list")
        );
    }

    #[test]
    fn a_repeated_flag_wins_with_its_last_value() {
        assert_eq!(
            run(&["--plugin", "hello", "--plugin", "settings"]).plugin,
            PluginSpec::Name("settings".into())
        );
    }

    #[test]
    fn unknown_flag_is_rejected_naming_the_flag() {
        let message = err(&["--frob"]);
        assert!(message.contains("--frob"), "got: {message}");
        assert!(message.contains("known flags"), "got: {message}");
    }

    #[test]
    fn a_flag_value_may_not_look_like_a_flag() {
        let message = err(&["--plugin", "--png"]);
        assert!(
            message.contains("`--plugin` needs a value"),
            "got: {message}"
        );
    }

    #[test]
    fn missing_flag_value_is_rejected() {
        let message = err(&["--plugin"]);
        assert!(
            message.contains("`--plugin` needs a value"),
            "got: {message}"
        );
        let message = err(&["--command"]);
        assert!(
            message.contains("`--command` needs a value"),
            "got: {message}"
        );
    }

    #[test]
    fn empty_flag_value_is_rejected() {
        let message = err(&["--plugin", ""]);
        assert!(message.contains("non-empty"), "got: {message}");
    }

    #[test]
    fn positional_arguments_are_rejected() {
        let message = err(&["settings"]);
        assert!(
            message.contains("unexpected argument `settings`"),
            "got: {message}"
        );
    }

    #[test]
    fn arguments_after_a_bare_separator_are_rejected() {
        let message = err(&["--", "--png"]);
        assert!(
            message.contains("unexpected argument `--png`"),
            "got: {message}"
        );
    }

    #[test]
    fn help_flag_yields_the_usage_text() {
        for help in ["--help", "-h"] {
            let argv = [help.to_owned()];
            match parse(&argv).expect("parse") {
                Parsed::Help(text) => {
                    assert!(text.contains("--plugin"), "got: {text}");
                    assert!(text.contains("Examples"), "got: {text}");
                }
                Parsed::Run(_) => panic!("`{help}` must request help"),
            }
        }
    }
}
