//! Operator CLI for the external plugin catalog.
//!
//! Only an operator with server access can add a privileged plugin: `add`
//! checks the token-grant agreement, installs the verified binary, and writes
//! the catalog entry. `list` prints the catalog. `remove` drops the catalog
//! entry and leaves the installed binary on disk.
//!
//! Paths follow the host: `PLUGINS_TOML` or `DATA_PATH/plugins.toml`, and
//! `PLUGINS_DIR` or `DATA_PATH/plugins` (`DATA_PATH` defaults to `./data`).

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Subcommand;
use pwr_plugin_protocol::Manifest;

use crate::config::Config;
use crate::plugin::CatalogEntry;
use crate::plugin::PluginCatalog;
use crate::plugin::catalog_writer::check_grant;
use crate::plugin::catalog_writer::remove_entry;
use crate::plugin::catalog_writer::upsert_entry;
use crate::plugin::install::download_client;
use crate::plugin::install::install_verified;
use crate::plugin::install::validate_entry;

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Install a verified plugin and write its catalog entry
    ///
    /// Example: pwr-bot plugin add --name feed --url https://example.test/feed
    ///          --sha256 <hex> --manifest ./manifest.json
    Add {
        /// Plugin name; must match the name declared in the manifest
        #[arg(long)]
        name: String,
        /// HTTPS URL of the verified plugin binary
        #[arg(long)]
        url: String,
        /// Expected hex SHA-256 digest of the binary
        #[arg(long)]
        sha256: String,
        /// Path to the plugin manifest JSON file
        #[arg(long)]
        manifest: PathBuf,
        /// Grant the Discord token; the manifest must declare `discord_token`
        #[arg(long)]
        grant_token: bool,
        /// Enable the plugin in the catalog when the install completes
        #[arg(long)]
        auto_enable: bool,
    },
    /// List catalog entries
    List,
    /// Remove a catalog entry, leaving the binary on disk
    Remove {
        #[arg(long)]
        name: String,
    },
}

#[derive(Debug)]
pub struct AddOptions {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub manifest_path: PathBuf,
    pub grant_token: bool,
    pub auto_enable: bool,
}

/// Resolves the catalog and install paths without loading `Config`.
pub fn paths() -> (PathBuf, PathBuf) {
    let data_path = PathBuf::from(std::env::var("DATA_PATH").unwrap_or_else(|_| "./data".into()));
    let config = Config {
        plugins_toml: std::env::var("PLUGINS_TOML")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_path.join("plugins.toml")),
        plugins_dir: std::env::var("PLUGINS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_path.join("plugins")),
        ..Default::default()
    };
    (config.plugins_toml, config.plugins_dir)
}

pub async fn run(action: PluginCommand) -> Result<()> {
    let (plugins_toml, plugins_dir) = paths();
    match action {
        PluginCommand::Add {
            name,
            url,
            sha256,
            manifest,
            grant_token,
            auto_enable,
        } => {
            add(
                &plugins_toml,
                &plugins_dir,
                &AddOptions {
                    name,
                    url,
                    sha256,
                    manifest_path: manifest,
                    grant_token,
                    auto_enable,
                },
            )
            .await
        }
        PluginCommand::List => list(&plugins_toml),
        PluginCommand::Remove { name } => remove(&plugins_toml, &name),
    }
}

async fn add(plugins_toml: &Path, plugins_dir: &Path, options: &AddOptions) -> Result<()> {
    let AddOptions {
        name,
        url,
        sha256,
        manifest_path,
        grant_token,
        auto_enable,
    } = options;
    let text = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read manifest `{}`", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse manifest `{}`", manifest_path.display()))?;
    manifest.validate().context("manifest failed validation")?;
    if manifest.name != *name {
        bail!(
            "manifest names itself `{}` but --name is `{name}`",
            manifest.name
        );
    }
    check_grant(*grant_token, &manifest)?;

    let entry = CatalogEntry {
        name: name.clone(),
        url: url.clone(),
        sha256: sha256.clone(),
        manifest,
        auto_enable: *auto_enable,
        discord_token: *grant_token,
    };

    validate_entry(&entry, plugins_toml).context("catalog entry failed validation")?;
    let target = plugins_dir.join(name);
    if target.parent() != Some(plugins_dir) {
        bail!("plugin name `{name}` escapes the plugins directory");
    }

    fs::create_dir_all(plugins_dir)
        .with_context(|| format!("failed to create plugins dir `{}`", plugins_dir.display()))?;
    let installed = install_verified(&download_client(), &entry, plugins_dir)
        .await
        .context("plugin binary install failed")?;
    upsert_entry(plugins_toml, &entry)
        .with_context(|| format!("failed to write catalog `{}`", plugins_toml.display()))?;

    println!("installed plugin {name} at {}", installed.display());
    println!(
        "AUDIT plugin {name}: discord_token grant {} (manifest declares it: {})",
        if *grant_token { "ENABLED" } else { "DISABLED" },
        entry.manifest.requires_discord_token()
    );
    Ok(())
}

fn list(plugins_toml: &Path) -> Result<()> {
    let catalog = PluginCatalog::load(plugins_toml)
        .with_context(|| format!("failed to load catalog `{}`", plugins_toml.display()))?;
    if catalog.is_empty() {
        println!("no plugin entries in {}", plugins_toml.display());
        return Ok(());
    }
    let mut entries: Vec<_> = catalog.values().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    for entry in entries {
        println!(
            "{}\t{}\t{}\tauto_enable={}\tdiscord_token={}",
            entry.name, entry.url, entry.sha256, entry.auto_enable, entry.discord_token
        );
    }
    Ok(())
}

fn remove(plugins_toml: &Path, name: &str) -> Result<()> {
    let removed = remove_entry(plugins_toml, name)
        .with_context(|| format!("failed to edit catalog `{}`", plugins_toml.display()))?;
    if removed {
        println!("removed catalog entry for plugin {name}; binary left on disk");
        println!("AUDIT plugin {name}: catalog entry removed (discord_token grant revoked)");
    } else {
        println!("no catalog entry named `{name}`; nothing removed");
    }
    Ok(())
}
