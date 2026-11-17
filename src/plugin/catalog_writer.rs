//! Operator-side catalog writer: edits one `[[plugins]]` entry in a
//! `plugins.toml` file without disturbing the rest of the document.
//!
//! The catalog is operator-owned configuration (ADR-0016). Comments, key
//! layout, and every other entry survive an add, replace, or remove: only
//! the named array-of-tables element is touched.
//!
//! [`check_grant`] enforces the token-grant agreement before an entry is
//! written: the operator's flag and the manifest's declaration must say the
//! same thing in both directions.

use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use pwr_plugin_protocol::Manifest;
use tempfile::NamedTempFile;
use toml_edit::ArrayOfTables;
use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;
use toml_edit::Value;

use crate::plugin::CatalogEntry;

/// A failure while reading, parsing, or writing the catalog.
#[derive(Debug, thiserror::Error)]
pub enum CatalogWriterError {
    #[error("failed to read catalog `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse catalog `{path}`: {detail}")]
    Parse { path: PathBuf, detail: String },
    #[error("catalog `{path}` has a `plugins` key that is not an array of tables")]
    Shape { path: PathBuf },
    #[error("failed to serialize the plugin manifest: {source}")]
    Manifest {
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write catalog `{path}`: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("token grant and manifest declaration disagree for `{name}`: {mismatch}")]
    GrantAgreement {
        name: String,
        mismatch: GrantMismatch,
    },
}

/// The two ways the token grant and the manifest declaration can disagree.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum GrantMismatch {
    #[error("token granted but the manifest does not declare `discord_token`")]
    GrantWithoutDeclaration,
    #[error("manifest declares `discord_token` but the operator did not grant it")]
    DeclarationWithoutGrant,
}

/// Checks the token-grant agreement (ADR-0016): the operator's flag and the
/// manifest's `requires` declaration must agree in both directions.
pub fn check_grant(grant_token: bool, manifest: &Manifest) -> Result<(), GrantMismatch> {
    match (grant_token, manifest.requires_discord_token()) {
        (true, false) => Err(GrantMismatch::GrantWithoutDeclaration),
        (false, true) => Err(GrantMismatch::DeclarationWithoutGrant),
        _ => Ok(()),
    }
}

/// Adds `entry`, or replaces the existing entry with the same name. Only the
/// named `[[plugins]]` element changes; comments and other entries survive.
pub fn upsert_entry(path: &Path, entry: &CatalogEntry) -> Result<(), CatalogWriterError> {
    check_grant(entry.discord_token, &entry.manifest).map_err(|mismatch| {
        CatalogWriterError::GrantAgreement {
            name: entry.name.clone(),
            mismatch,
        }
    })?;
    let mut doc = load(path)?;
    let fields = entry_fields(entry)?;

    let trailing = if doc.get("plugins").is_none() {
        let text = doc.trailing().as_str().map(str::to_string);
        if text.is_some() {
            doc.set_trailing("");
        }
        doc.insert("plugins", Item::ArrayOfTables(ArrayOfTables::new()));
        text
    } else {
        None
    };

    let Some(array) = doc
        .get_mut("plugins")
        .and_then(Item::as_array_of_tables_mut)
    else {
        return Err(CatalogWriterError::Shape {
            path: path.to_path_buf(),
        });
    };

    if let Some(table) = array
        .iter_mut()
        .find(|t| t.get("name").and_then(Item::as_str) == Some(entry.name.as_str()))
    {
        for (key, val) in fields {
            set_field(table, key, val);
        }
    } else {
        let mut table = Table::new();
        for (key, val) in fields {
            table.insert(key, Item::Value(val));
        }
        array.push(table);
    }
    if let Some(text) = trailing {
        let first = array.get_mut(0).expect("the array was just pushed");
        let existing = first
            .decor()
            .prefix()
            .map(|r| r.as_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        first.decor_mut().set_prefix(format!("{text}{existing}"));
    }

    write(path, &doc)
}

/// Removes the `[[plugins]]` entry named `name`, leaving the installed binary
/// and every other entry alone. Returns whether an entry was removed.
pub fn remove_entry(path: &Path, name: &str) -> Result<bool, CatalogWriterError> {
    let mut doc = load(path)?;
    if doc.get("plugins").is_none() {
        return Ok(false);
    }
    let Some(array) = doc
        .get_mut("plugins")
        .and_then(Item::as_array_of_tables_mut)
    else {
        return Err(CatalogWriterError::Shape {
            path: path.to_path_buf(),
        });
    };
    let Some(index) = array
        .iter()
        .position(|t| t.get("name").and_then(Item::as_str) == Some(name))
    else {
        return Ok(false);
    };
    let prefix = array
        .get(index)
        .and_then(|t| t.decor().prefix())
        .map(|r| r.as_str().unwrap_or_default().to_string());
    array.remove(index);
    let has_next = index < array.len();

    if let Some(prefix) = prefix {
        if has_next {
            let array = doc
                .get_mut("plugins")
                .and_then(Item::as_array_of_tables_mut)
                .expect("plugins is an array of tables");
            let next = array
                .get_mut(index)
                .expect("a table follows the removed one");
            let existing = next
                .decor()
                .prefix()
                .map(|r| r.as_str().unwrap_or_default().to_string())
                .unwrap_or_default();
            next.decor_mut().set_prefix(format!("{prefix}{existing}"));
        } else {
            let existing = doc
                .decor()
                .prefix()
                .map(|r| r.as_str().unwrap_or_default().to_string())
                .unwrap_or_default();
            doc.decor_mut().set_prefix(format!("{existing}{prefix}"));
        }
    }

    write(path, &doc)?;
    Ok(true)
}

fn load(path: &Path) -> Result<DocumentMut, CatalogWriterError> {
    let text = fs::read_to_string(path).map_err(|source| CatalogWriterError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    text.parse::<DocumentMut>()
        .map_err(|detail| CatalogWriterError::Parse {
            path: path.to_path_buf(),
            detail: detail.to_string(),
        })
}

fn write(path: &Path, doc: &DocumentMut) -> Result<(), CatalogWriterError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(parent).map_err(|source| CatalogWriterError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    temp.write_all(doc.to_string().as_bytes())
        .map_err(|source| CatalogWriterError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    temp.persist(path).map_err(|e| CatalogWriterError::Write {
        path: path.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

fn entry_fields(entry: &CatalogEntry) -> Result<Vec<(&'static str, Value)>, CatalogWriterError> {
    let manifest = serde_json::to_string(&entry.manifest)
        .map_err(|source| CatalogWriterError::Manifest { source })?;
    Ok(vec![
        ("name", Value::from(entry.name.clone())),
        ("url", Value::from(entry.url.clone())),
        ("sha256", Value::from(entry.sha256.clone())),
        ("manifest", Value::from(manifest)),
        ("auto_enable", Value::from(entry.auto_enable)),
        ("discord_token", Value::from(entry.discord_token)),
    ])
}

fn set_field(table: &mut Table, key: &str, val: Value) {
    let mut val = val;
    if let Some(existing) = table.get_mut(key).and_then(Item::as_value_mut) {
        *val.decor_mut() = existing.decor().clone();
        *existing = val;
    } else {
        table.insert(key, Item::Value(val));
    }
}

#[cfg(test)]
mod tests {
    use pwr_plugin_protocol::CommandDef;
    use tempfile::tempdir;

    use super::*;
    use crate::plugin::PluginCatalog;

    const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn manifest_named(name: &str, requires_token: bool) -> Manifest {
        Manifest {
            name: name.into(),
            description: format!("{name} plugin"),
            version: "0.1.0".into(),
            commands: vec![CommandDef {
                create_command: serde_json::json!({
                    "name": name,
                    "description": format!("Run {name}"),
                }),
            }],
            event_handlers: vec![],
            tasks: vec![],
            settings: vec![],
            requires: if requires_token {
                vec![pwr_plugin_protocol::DISCORD_TOKEN.into()]
            } else {
                vec![]
            },
            api_version: pwr_plugin_protocol::API_VERSION,
        }
    }

    fn entry(name: &str, url: &str, sha: &str, grant: bool) -> CatalogEntry {
        CatalogEntry {
            name: name.into(),
            url: url.into(),
            sha256: sha.into(),
            manifest: manifest_named(name, grant),
            auto_enable: false,
            discord_token: grant,
        }
    }

    /// A catalog with a top comment, two entries, and per-entry comments.
    fn seeded_catalog(path: &Path) {
        let a = serde_json::to_string(&manifest_named("alpha", false)).unwrap();
        let b = serde_json::to_string(&manifest_named("beta", false)).unwrap();
        fs::write(
            path,
            format!(
                "# operator-managed catalog\n\
                 [[plugins]]\n\
                 name = \"alpha\" # the first plugin\n\
                 url = \"https://example.com/alpha\"\n\
                 sha256 = \"{SHA_A}\"\n\
                 manifest = '{a}'\n\
                 \n\
                 # a second plugin\n\
                 [[plugins]]\n\
                 name = \"beta\"\n\
                 url = \"https://example.com/beta\"\n\
                 sha256 = \"{SHA_B}\"\n\
                 manifest = '{b}'\n\
                 # end of catalog\n"
            ),
        )
        .unwrap();
    }

    fn names(path: &Path) -> Vec<String> {
        let doc: DocumentMut = fs::read_to_string(path).unwrap().parse().unwrap();
        doc["plugins"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    // ── writer seams ────────────────────────────────────────────────────────

    #[test]
    fn add_appends_an_entry_and_preserves_comments_and_others() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        seeded_catalog(&path);
        let before = fs::read_to_string(&path).unwrap();

        upsert_entry(
            &path,
            &entry(
                "gamma",
                "https://example.com/gamma",
                &"cc".repeat(32),
                false,
            ),
        )
        .unwrap();

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(names(&path), ["alpha", "beta", "gamma"]);
        assert!(before.lines().all(|line| after.contains(line)));
        let catalog = PluginCatalog::load(&path).unwrap();
        assert_eq!(catalog["gamma"].url, "https://example.com/gamma");
    }

    #[test]
    fn replace_updates_one_entry_and_leaves_the_other_untouched() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        seeded_catalog(&path);

        upsert_entry(
            &path,
            &entry(
                "alpha",
                "https://example.com/alpha-v2",
                &"dd".repeat(32),
                false,
            ),
        )
        .unwrap();

        let catalog = PluginCatalog::load(&path).unwrap();
        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog["alpha"].url, "https://example.com/alpha-v2");
        assert_eq!(catalog["alpha"].sha256, "dd".repeat(32));
        assert_eq!(catalog["beta"].url, "https://example.com/beta");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# the first plugin"));
        assert!(text.contains("# a second plugin"));
    }

    #[test]
    fn remove_deletes_only_the_named_entry() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        seeded_catalog(&path);

        assert!(remove_entry(&path, "alpha").unwrap());

        assert_eq!(names(&path), ["beta"]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# operator-managed catalog"));
        assert!(text.contains("# a second plugin"));
        assert!(text.contains("# end of catalog"));
    }

    #[test]
    fn remove_reports_false_for_an_unknown_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        seeded_catalog(&path);
        let before = fs::read_to_string(&path).unwrap();

        assert!(!remove_entry(&path, "missing").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn add_creates_the_plugins_array_when_the_catalog_is_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(&path, "# catalog header\n").unwrap();

        upsert_entry(
            &path,
            &entry("gamma", "https://example.com/gamma", SHA_A, false),
        )
        .unwrap();

        assert_eq!(names(&path), ["gamma"]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# catalog header"));
    }

    // ── grant agreement seam ────────────────────────────────────────────────

    #[test]
    fn grant_and_declaration_agree_in_both_directions() {
        let declared = manifest_named("pro", true);
        let plain = manifest_named("plain", false);
        assert!(check_grant(true, &declared).is_ok());
        assert!(check_grant(false, &plain).is_ok());
    }

    #[test]
    fn upsert_entry_rejects_a_grant_without_a_declaration_and_leaves_the_file_untouched() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        seeded_catalog(&path);
        let before = fs::read_to_string(&path).unwrap();
        let mut mismatched = entry("voice", "https://example.com/voice", SHA_A, false);
        mismatched.discord_token = true;

        assert!(matches!(
            upsert_entry(&path, &mismatched),
            Err(CatalogWriterError::GrantAgreement {
                name,
                mismatch: GrantMismatch::GrantWithoutDeclaration,
            }) if name == "voice"
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn upsert_entry_rejects_a_declaration_without_a_grant_and_leaves_the_file_untouched() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        seeded_catalog(&path);
        let before = fs::read_to_string(&path).unwrap();
        let mut mismatched = entry("voice", "https://example.com/voice", SHA_A, true);
        mismatched.discord_token = false;

        assert!(matches!(
            upsert_entry(&path, &mismatched),
            Err(CatalogWriterError::GrantAgreement {
                name,
                mismatch: GrantMismatch::DeclarationWithoutGrant,
            }) if name == "voice"
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn grant_without_a_declaration_is_rejected() {
        let manifest = manifest_named("plain", false);
        assert_eq!(
            check_grant(true, &manifest),
            Err(GrantMismatch::GrantWithoutDeclaration)
        );
    }

    #[test]
    fn declaration_without_a_grant_is_rejected() {
        let manifest = manifest_named("pro", true);
        assert_eq!(
            check_grant(false, &manifest),
            Err(GrantMismatch::DeclarationWithoutGrant)
        );
    }
}
