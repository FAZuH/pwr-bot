use std::process::Command;
use std::process::Output;

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pwr-bot"))
        .args(args)
        .output()
        .expect("failed to run pwr-bot")
}

#[test]
fn help_lists_the_command_tree() {
    let output = run(&["--help"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in ["serve", "plugin", "service", "version", "debug"] {
        assert!(stdout.contains(name), "help is missing `{name}`");
    }
}

#[test]
fn version_subcommand_prints_the_package_version() {
    let output = run(&["version"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn plugin_help_lists_catalog_commands() {
    let output = run(&["plugin", "--help"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for name in ["add", "list", "remove"] {
        assert!(stdout.contains(name), "plugin help is missing `{name}`");
    }
}

#[test]
fn debug_paths_prints_all_resolved_paths_without_loading_config() {
    let output = run(&["debug", "paths"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for label in ["config:", "data:", "log:", "registration:"] {
        assert!(stdout.contains(label), "debug paths is missing `{label}`");
    }
}
