//! Preview feedback loop: spawn a plugin, invoke one of its commands, and
//! render the returned view to inspectable files under `.scratch/preview/`.
//!
//! This binary is an Adapter over the plugin wire protocol
//! (`pwr-plugin-protocol`): it speaks the same JSON-Lines-over-stdio
//! protocol as the host, using the protocol crate's own framing types
//! (`Msg`, `ViewSpec`, `Manifest`) — no wire parsing is reimplemented and
//! nothing is imported from the host crate. Everything is synchronous std
//! stdio (no tokio): the tool makes one call per run. A plugin that never
//! answers blocks this tool until interrupted (Ctrl-C reaches the plugin
//! too — it is not detached into its own process group).
//!
//! # Flow
//!
//! locate the plugin binary → spawn → hello handshake (version, caps, and
//! manifest validated via `pwr_plugin_protocol`) → ack with the preview's
//! own hello → `invoke` the command → interpret the resp payload as the
//! view message (envelope or raw v1 shape, see
//! [`proto::message_from_resp_data`]) → write it as a fixture JSON file →
//! `pwr_viewgen` parses, validates, and renders it to HTML (and optionally
//! PNG).
//!
//! # Binary resolution
//!
//! 1. `--plugin <path>`: a flag value containing a path separator is used
//!    verbatim (override).
//! 2. `--plugin <name>` or the default (`hello`): a sibling of this
//!    executable in the same `target/<profile>/` directory first, then the
//!    `debug` and `release` profile directories of the same `target/`
//!    (fallback) — the layout `tests/probe.rs` probes for the integration
//!    suites. A standalone binary has no `CARGO_BIN_EXE_*`, so the build
//!    output next to it is the reliable anchor.
//! 3. Nothing found: the error lists every path tried and the `cargo build`
//!    that produces them.
//!
//! # PNG feature wiring
//!
//! PNG rendering needs headless Chromium (`chromiumoxide`), a heavy
//! dependency tree, so it is opt-in at build time: this crate's `png`
//! feature forwards to pwr-viewgen's `png` feature, and this crate depends
//! on pwr-viewgen with `default-features = false` so the default build
//! stays light. `./dev.sh preview` detects `--png` and builds with
//! `--features pwr-preview/png`; a binary built without the feature fails
//! `--png` with the rebuild hint instead of rendering. The workspace
//! resolver keeps pwr-viewgen's `png`/`cli` defaults out of normal builds
//! of this crate.
//!
//! # Fixtures
//!
//! Files land in `<current dir>/.scratch/preview/`, which is gitignored:
//! `<plugin>-<command>.json` (the exact payload the render pipeline read),
//! `<plugin>-<command>.html`, and — with `--png` —
//! `<plugin>-<command>.png`. Run from the workspace root (`./dev.sh
//! preview` does this) so they land in the repo's `.scratch/`.

use std::process::ExitCode;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use anyhow::Context;
use anyhow::Result;

use crate::args::Parsed;
use crate::args::PreviewArgs;
use crate::fixture::FixturePaths;
use crate::fixture::fixture_paths;
use crate::proto::PluginSession;
use crate::proto::message_from_resp_data;
use crate::proto::single_command_name;
use crate::resolve::resolve;

mod args;
mod fixture;
mod proto;
mod resolve;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let outcome = match args::parse(&argv) {
        Ok(Parsed::Help(usage)) => {
            println!("{usage}");
            Ok(())
        }
        Ok(Parsed::Run(run)) => run_preview(&run),
        Err(message) => Err(anyhow::anyhow!("{message}")),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Runs one preview capture. See the crate docs for the flow.
fn run_preview(args: &PreviewArgs) -> Result<()> {
    let exe = std::env::current_exe().context("locating the preview binary")?;
    let plugin_path = resolve(&exe, &args.plugin)
        .with_context(|| format!("resolving the `{}` plugin binary", args.plugin))?;
    println!("plugin:  {} ({})", args.plugin, plugin_path.display());

    let mut session = PluginSession::spawn(&plugin_path).context("starting the plugin")?;
    let command = match &args.command {
        Some(command) => command.clone(),
        None => single_command_name(session.manifest.as_ref()).with_context(|| {
            format!(
                "the `{}` plugin declares no single command to invoke; pass --command <name>",
                session.name
            )
        })?,
    };
    println!("command: {command}");
    let payload = session
        .invoke(&command, serde_json::json!({}))
        .with_context(|| format!("invoking the `{command}` command"))?;
    let message = message_from_resp_data(&payload);
    let plugin_name = session.name.clone();
    session.shutdown().context("stopping the plugin")?;

    let paths = fixture_paths(&current_dir()?, &plugin_name, &command);
    std::fs::create_dir_all(&paths.dir)
        .with_context(|| format!("creating the fixture directory {}", paths.dir.display()))?;
    let json = serde_json::to_string_pretty(&message).context("serializing the view fixture")?;
    std::fs::write(&paths.json, &json)
        .with_context(|| format!("writing the fixture {}", paths.json.display()))?;
    println!("fixture: {}", paths.json.display());

    let parsed = pwr_viewgen::model::parse_message(&json).context("parsing the view fixture")?;
    pwr_viewgen::validate::validate(&parsed.message)
        .context("the plugin view is not renderable")?;
    let html = pwr_viewgen::render::render_html(&parsed.message, now_unix()?);
    std::fs::write(&paths.html, &html)
        .with_context(|| format!("writing the preview {}", paths.html.display()))?;
    println!("html:    {}", paths.html.display());

    if args.png {
        render_png(&paths, &html)?;
    }
    Ok(())
}

/// Renders the PNG for an already-written HTML preview. PNG is secondary to
/// the HTML deliverable: any capture failure — headless Chromium missing
/// (see `PWR_VIEWGEN_CHROME`) included — is reported as an environment
/// note on stderr and the run still succeeds.
#[cfg(feature = "png")]
fn render_png(paths: &FixturePaths, html: &str) -> Result<()> {
    let bytes = pwr_viewgen::shot::capture_html(
        html,
        pwr_viewgen::render::DEFAULT_CONTENT_WIDTH,
        PNG_SCALE,
    );
    match bytes {
        Ok(bytes) => {
            std::fs::write(&paths.png, bytes)
                .with_context(|| format!("writing the PNG {}", paths.png.display()))?;
            println!("png:     {}", paths.png.display());
            Ok(())
        }
        Err(error) => {
            eprintln!("note: PNG skipped: {error}");
            Ok(())
        }
    }
}

/// Device scale factor for the optional PNG render, matching pwr-viewgen's
/// own CLI default.
#[cfg(feature = "png")]
const PNG_SCALE: f32 = 2.0;

/// Rejects `--png` in a build without the `png` feature with the rebuild
/// hint instead of silently skipping the request.
#[cfg(not(feature = "png"))]
fn render_png(_paths: &FixturePaths, _html: &str) -> Result<()> {
    anyhow::bail!(
        "--png needs a build with the `png` feature; rebuild with \
         `cargo build -p pwr-preview --features png`"
    );
}

/// The current unix time in whole seconds, feeding relative timestamps in
/// the rendered HTML.
fn now_unix() -> Result<i64> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is set before the unix epoch")?;
    Ok(now.as_secs() as i64)
}

/// The directory fixtures are written relative to.
fn current_dir() -> Result<std::path::PathBuf> {
    std::env::current_dir().context("getting the current directory")
}
