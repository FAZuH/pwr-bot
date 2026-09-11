//! External plugin install: a `plugins.toml` catalog mapping plugin names to
//! https download URLs pinned by sha256, then download → verify → install
//! into the plugins directory.
//!
//! [`PluginCatalog::load`] parses and validates the catalog.
//! [`install_verified`] runs the full pipeline and returns the installed
//! binary path, ready for
//! [`PluginManager::spawn`](crate::plugin::PluginManager::spawn).

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use futures::StreamExt;
use log::warn;
use pwr_plugin_protocol::Manifest;
use serde::Deserialize;
use sha2::Digest;
use sha2::Sha256;
use tempfile::NamedTempFile;
use wreq::Url;

use crate::plugin::InstallError;

/// Maximum redirect hops a download may follow before giving up.
const MAX_REDIRECTS: usize = 5;

/// How long a download may take before it is abandoned.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// A download body larger than this is rejected before it is fully read;
/// a plugin binary should never approach 128 MiB.
const MAX_DOWNLOAD_BYTES: u64 = 128 * 1024 * 1024;

/// One pinned plugin in the catalog: name, https download url, the sha256
/// the binary must match, and the plugin's manifest. The manifest is
/// embedded in `plugins.toml` as a JSON string (its command blobs are
/// Discord `CreateCommand` JSON), so the host can register a plugin's slash
/// commands and validate it before install.
#[derive(Debug, Clone, Deserialize)]
pub struct CatalogEntry {
    /// Plugin name; also the installed binary's file name.
    pub name: String,
    /// https download URL.
    pub url: String,
    /// Expected sha256 of the binary, 64 lowercase hex chars.
    pub sha256: String,
    /// The plugin's manifest, as declared in the catalog.
    #[serde(deserialize_with = "deserialize_manifest")]
    pub manifest: Manifest,
    /// Whether the plugin is enabled in every guild by default (an absent
    /// `guild_plugins` row means enabled). `false` by default.
    #[serde(default)]
    pub auto_enable: bool,
}

/// Deserializes a catalog `manifest` field: the manifest is embedded in
/// `plugins.toml` as a JSON string, since a `CreateCommand` blob is Discord
/// JSON and would not survive a TOML round-trip.
fn deserialize_manifest<'de, D>(deserializer: D) -> Result<Manifest, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    serde_json::from_str(&text).map_err(serde::de::Error::custom)
}

/// The `plugins.toml` catalog: a top-level `[[plugins]]` array of tables.
#[derive(Debug, Deserialize)]
struct CatalogFile {
    plugins: Vec<CatalogEntry>,
}

/// Loads the plugin catalog from `plugins.toml` and validates its entries.
#[derive(Debug)]
pub struct PluginCatalog;

impl PluginCatalog {
    /// Parses the catalog at `path` and returns its entries keyed by name.
    /// Every entry is validated (non-empty name, https url, 64-hex sha256,
    /// valid manifest) and duplicate names are rejected.
    pub fn load(path: &Path) -> Result<HashMap<String, CatalogEntry>, InstallError> {
        let text = fs::read_to_string(path).map_err(|source| InstallError::Catalog {
            path: path.to_path_buf(),
            detail: source.to_string(),
        })?;
        let file: CatalogFile = toml::from_str(&text).map_err(|detail| InstallError::Catalog {
            path: path.to_path_buf(),
            detail: detail.to_string(),
        })?;
        let mut entries = HashMap::with_capacity(file.plugins.len());
        for entry in file.plugins {
            validate_entry(&entry, path)?;
            if entries.contains_key(&entry.name) {
                return Err(InstallError::Catalog {
                    path: path.to_path_buf(),
                    detail: format!("duplicate plugin name `{}`", entry.name),
                });
            }
            entries.insert(entry.name.clone(), entry);
        }
        Ok(entries)
    }
}

/// The host's plugin-download client: https-only with an explicit redirect
/// policy. Tests build their own client against a local http mock server,
/// so the https-only enforcement is asserted separately.
pub fn download_client() -> wreq::Client {
    wreq::Client::builder()
        .https_only(true)
        .redirect(redirect_policy())
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .expect("wreq client construction cannot fail")
}

/// The download redirect policy: follow at most [`MAX_REDIRECTS`] hops, and
/// only onto https URLs; anything else stops the redirect. The client's
/// `https_only` flag is the second layer.
pub fn redirect_policy() -> wreq::redirect::Policy {
    wreq::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() > MAX_REDIRECTS {
            attempt.error("too many redirects")
        } else if attempt.url().scheme() != "https" {
            attempt.stop()
        } else {
            attempt.follow()
        }
    })
}

/// Downloads `entry`'s binary into a fresh temp file inside `plugin_dir`.
/// The response body is streamed chunk-by-chunk with a
/// [`MAX_DOWNLOAD_BYTES`] cap, so an oversized body is rejected without
/// being buffered in memory first; the temp file is deleted on drop, so an
/// aborted install leaves nothing behind.
pub async fn download(
    client: &wreq::Client,
    entry: &CatalogEntry,
    plugin_dir: &Path,
) -> Result<NamedTempFile, InstallError> {
    let resp = client
        .get(&entry.url)
        .send()
        .await
        .map_err(|source| InstallError::Download {
            name: entry.name.clone(),
            url: entry.url.clone(),
            source,
        })?;
    let resp = resp
        .error_for_status()
        .map_err(|source| InstallError::Download {
            name: entry.name.clone(),
            url: entry.url.clone(),
            source,
        })?;
    let mut file = NamedTempFile::new_in(plugin_dir).map_err(|source| InstallError::Io {
        path: plugin_dir.to_path_buf(),
        source,
    })?;
    let mut stream = Box::pin(resp.bytes_stream());
    let mut size: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| InstallError::Download {
            name: entry.name.clone(),
            url: entry.url.clone(),
            source,
        })?;
        size = size.saturating_add(chunk.len() as u64);
        if size > MAX_DOWNLOAD_BYTES {
            return Err(InstallError::TooLarge {
                name: entry.name.clone(),
                url: entry.url.clone(),
                size,
                max: MAX_DOWNLOAD_BYTES,
            });
        }
        file.write_all(&chunk).map_err(|source| InstallError::Io {
            path: file.path().to_path_buf(),
            source,
        })?;
    }
    Ok(file)
}

/// The lowercase hex sha256 of the file at `path`, streamed in 64 KiB chunks.
pub fn sha256_hex(path: &Path) -> Result<String, InstallError> {
    let mut file = fs::File::open(path).map_err(|source| InstallError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|source| InstallError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    Ok(hex)
}

/// Verifies the file at `path` against the entry's pinned sha256
/// (case-insensitive hex). The pin is trimmed before comparing, matching
/// the validation in [`PluginCatalog::load`], so a pinned sha with stray
/// whitespace in `plugins.toml` still verifies.
pub fn verify(entry: &CatalogEntry, path: &Path) -> Result<(), InstallError> {
    let got = sha256_hex(path)?;
    if got.eq_ignore_ascii_case(entry.sha256.trim()) {
        Ok(())
    } else {
        Err(InstallError::Verify {
            name: entry.name.clone(),
            expected: entry.sha256.clone(),
            got,
        })
    }
}

/// Atomically installs the verified temp file at `target`: refuses symlink
/// targets, validates the ELF machine type before anything lands on disk,
/// chmods to `0o755` before `persist()` (the rename preserves the mode, so
/// the installed file never exists with the wrong permissions), then
/// `persist()`s (atomic rename).
pub fn install(
    temp: NamedTempFile,
    entry: &CatalogEntry,
    target: &Path,
) -> Result<(), InstallError> {
    // Refuse symlink targets: persist() would follow the link and write
    // outside the plugins dir.
    match fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(InstallError::Symlink {
                path: target.to_path_buf(),
            });
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(InstallError::Io {
                path: target.to_path_buf(),
                source,
            });
        }
    }
    check_elf(entry, temp.path())?;
    // chmod the temp file before the rename: persist() moves the inode, and
    // rename preserves its mode, so a 0o755 temp lands as a 0o755 install
    // with no window where the target exists with wrong permissions.
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o755)).map_err(|source| {
        InstallError::Io {
            path: temp.path().to_path_buf(),
            source,
        }
    })?;
    temp.persist(target).map_err(|e| InstallError::Install {
        name: entry.name.clone(),
        path: target.to_path_buf(),
        source: e.error,
    })?;
    Ok(())
}

/// Full install pipeline: download → verify → install → re-verify the
/// installed bytes (closing the TOCTOU window between the temp-file verify
/// and the rename) → the installed path, ready for
/// [`PluginManager::spawn`](crate::plugin::PluginManager::spawn).
pub async fn install_verified(
    client: &wreq::Client,
    entry: &CatalogEntry,
    plugin_dir: &Path,
) -> Result<PathBuf, InstallError> {
    let temp = download(client, entry, plugin_dir).await?;
    verify(entry, temp.path())?;
    let target = plugin_dir.join(&entry.name);
    install(temp, entry, &target)?;
    // TOCTOU: the bytes on disk after the rename may differ from what was
    // verified in the temp file; re-verify the installed artifact before
    // handing the path to spawn. A mismatch must not leave the tainted
    // binary on disk, so it is removed before the error propagates.
    if let Err(err) = verify(entry, &target) {
        warn!(
            "plugin {} failed re-verification after install, removing tainted binary",
            entry.name
        );
        let _ = fs::remove_file(&target);
        return Err(err);
    }
    Ok(target)
}

/// The ELF `e_machine` id for the host architecture, if known.
fn host_elf_machine() -> Option<u16> {
    match std::env::consts::ARCH {
        "x86_64" => Some(62),
        "aarch64" => Some(183),
        _ => None,
    }
}

/// Rejects the binary at `path` unless it is a native ELF executable:
/// magic `\x7fELF` in the first 4 bytes and an `e_machine` (offset 18,
/// little-endian u16) matching the host architecture.
fn check_elf(entry: &CatalogEntry, path: &Path) -> Result<(), InstallError> {
    let Some(expected) = host_elf_machine() else {
        return Err(InstallError::UnsupportedHostArch {
            arch: std::env::consts::ARCH.to_string(),
        });
    };
    let mut file = fs::File::open(path).map_err(|source| InstallError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut header = [0u8; 20];
    file.read_exact(&mut header)
        .map_err(|source| InstallError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if &header[..4] != b"\x7fELF" {
        return Err(InstallError::NotElf {
            name: entry.name.clone(),
        });
    }
    let machine = u16::from_le_bytes([header[18], header[19]]);
    if machine != expected {
        return Err(InstallError::ArchMismatch {
            name: entry.name.clone(),
            got: machine,
            expected,
        });
    }
    Ok(())
}

/// Validates one catalog entry: non-empty name (and not `.`/`..`), https
/// url, 64-hex sha256, and a manifest that validates and names the entry.
fn validate_entry(entry: &CatalogEntry, path: &Path) -> Result<(), InstallError> {
    if entry.name.trim().is_empty()
        || entry.name.contains(['/', '\\'])
        || entry.name == "."
        || entry.name == ".."
    {
        return Err(InstallError::Catalog {
            path: path.to_path_buf(),
            detail: format!("plugin name `{}` is not a valid file name", entry.name),
        });
    }
    let url = Url::parse(&entry.url).map_err(|detail| InstallError::Catalog {
        path: path.to_path_buf(),
        detail: format!("plugin `{}` has an invalid url: {detail}", entry.name),
    })?;
    if url.scheme() != "https" {
        return Err(InstallError::Catalog {
            path: path.to_path_buf(),
            detail: format!(
                "plugin `{}` url must be https, got `{}`",
                entry.name, entry.url
            ),
        });
    }
    let sha = entry.sha256.trim();
    if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(InstallError::Catalog {
            path: path.to_path_buf(),
            detail: format!("plugin `{}` sha256 must be 64 hex chars", entry.name),
        });
    }
    if entry.manifest.name != entry.name {
        return Err(InstallError::Catalog {
            path: path.to_path_buf(),
            detail: format!(
                "plugin `{}` manifest names itself `{}`",
                entry.name, entry.manifest.name
            ),
        });
    }
    entry
        .manifest
        .validate()
        .map_err(|detail| InstallError::Catalog {
            path: path.to_path_buf(),
            detail: format!("plugin `{}` has an invalid manifest: {detail}", entry.name),
        })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    /// A minimal valid ELF header: magic plus the `e_machine` at offset 18.
    fn elf_bytes(machine: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 20];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes
    }

    /// A catalog entry that passes `validate_entry`.
    fn valid_entry() -> CatalogEntry {
        CatalogEntry {
            name: "hello".into(),
            url: "https://example.com/hello_plugin".into(),
            sha256: "ab".repeat(32),
            manifest: valid_manifest(),
            auto_enable: false,
        }
    }

    /// A manifest that validates against the host.
    fn valid_manifest() -> Manifest {
        Manifest {
            name: "hello".into(),
            description: "Canonical test plugin".into(),
            version: "0.1.0".into(),
            commands: vec![pwr_plugin_protocol::CommandDef {
                create_command: serde_json::json!({
                    "name": "hello",
                    "description": "Say hello from a plugin",
                }),
            }],
            event_handlers: vec!["view.timeout".into()],
            tasks: vec![],
            api_version: pwr_plugin_protocol::API_VERSION,
        }
    }

    /// The JSON-string form of [`valid_manifest`], as embedded in TOML.
    fn valid_manifest_json() -> String {
        serde_json::to_string(&valid_manifest()).unwrap()
    }

    #[test]
    fn catalog_parses_a_valid_array() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"https://example.com/hello\"\n",
                    "sha256 = \"{}\"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                valid_manifest_json()
            ),
        )
        .unwrap();
        let entries = PluginCatalog::load(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries["hello"].url, "https://example.com/hello");
    }

    #[test]
    fn catalog_accepts_a_sha_pin_with_surrounding_whitespace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"https://example.com/hello\"\n",
                    "sha256 = \"  {}  \"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                valid_manifest_json()
            ),
        )
        .unwrap();
        let entries = PluginCatalog::load(&path).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn catalog_rejects_an_empty_name() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"\"\nurl = \"https://example.com/hello\"\n",
                    "sha256 = \"{}\"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                valid_manifest_json()
            ),
        )
        .unwrap();
        let err = PluginCatalog::load(&path).unwrap_err();
        assert!(matches!(err, InstallError::Catalog { .. }));
    }

    #[test]
    fn catalog_rejects_an_http_url() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"http://example.com/hello\"\n",
                    "sha256 = \"{}\"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                valid_manifest_json()
            ),
        )
        .unwrap();
        let err = PluginCatalog::load(&path).unwrap_err();
        assert!(matches!(err, InstallError::Catalog { .. }));
    }

    #[test]
    fn catalog_rejects_a_bad_sha256() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"https://example.com/hello\"\n",
                    "sha256 = \"xyz\"\nmanifest = '{}'\n",
                ),
                valid_manifest_json()
            ),
        )
        .unwrap();
        let err = PluginCatalog::load(&path).unwrap_err();
        assert!(matches!(err, InstallError::Catalog { .. }));
    }

    #[test]
    fn catalog_rejects_duplicate_names() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"https://example.com/a\"\n",
                    "sha256 = \"{}\"\nmanifest = '{}'\n[[plugins]]\nname = \"hello\"\n",
                    "url = \"https://example.com/b\"\nsha256 = \"{}\"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                valid_manifest_json(),
                "cd".repeat(32),
                valid_manifest_json()
            ),
        )
        .unwrap();
        let err = PluginCatalog::load(&path).unwrap_err();
        assert!(matches!(err, InstallError::Catalog { .. }));
    }

    #[test]
    fn sha256_hex_matches_a_known_digest() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, b"hello").unwrap();
        // sha256("hello")
        assert_eq!(
            sha256_hex(&path).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn verify_accepts_a_matching_pin_case_insensitively() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, b"hello").unwrap();
        let entry = CatalogEntry {
            sha256: "2CF24DBA5FB0A30E26E83B2AC5B9E29E1B161E5C1FA7425E73043362938B9824".into(),
            ..valid_entry()
        };
        verify(&entry, &path).unwrap();
    }

    #[test]
    fn verify_rejects_a_mismatched_pin() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("f");
        fs::write(&path, b"hello").unwrap();
        let entry = CatalogEntry {
            sha256: "aa".repeat(32),
            ..valid_entry()
        };
        let err = verify(&entry, &path).unwrap_err();
        assert!(matches!(err, InstallError::Verify { .. }));
    }

    #[test]
    fn elf_check_accepts_the_host_architecture() {
        let Some(machine) = host_elf_machine() else {
            return;
        };
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugin");
        fs::write(&path, elf_bytes(machine)).unwrap();
        check_elf(&valid_entry(), &path).unwrap();
    }

    #[test]
    fn elf_check_rejects_a_foreign_architecture() {
        let Some(machine) = host_elf_machine() else {
            return;
        };
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugin");
        fs::write(&path, elf_bytes(machine + 1)).unwrap();
        let err = check_elf(&valid_entry(), &path).unwrap_err();
        assert!(matches!(err, InstallError::ArchMismatch { .. }));
    }

    #[test]
    fn elf_check_rejects_non_elf_magic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugin");
        fs::write(&path, [0u8; 20]).unwrap();
        let err = check_elf(&valid_entry(), &path).unwrap_err();
        assert!(matches!(err, InstallError::NotElf { .. }));
    }

    #[test]
    fn install_rejects_a_symlink_target() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path().join("plugin");
        symlink(dir.path().join("elsewhere"), &target).unwrap();
        let temp = tempfile::NamedTempFile::new().unwrap();
        let err = install(temp, &valid_entry(), &target).unwrap_err();
        assert!(matches!(err, InstallError::Symlink { .. }));
    }

    #[tokio::test]
    async fn download_client_is_https_only_by_config() {
        // The production client must refuse http at the URL level (builder
        // config), not only when the redirect policy sees a hop. wreq
        // rejects the request before any socket opens, so this test is
        // offline and deterministic.
        let client = download_client();
        let entry = CatalogEntry {
            url: "http://example.com/hello_plugin".into(),
            ..valid_entry()
        };
        let dir = tempdir().unwrap();
        let err = download(&client, &entry, dir.path()).await.unwrap_err();
        assert!(
            matches!(err, InstallError::Download { .. }),
            "https-only client must reject an http url without a request: {err}"
        );
    }

    #[test]
    fn validate_entry_rejects_dot_and_dotdot_names() {
        for name in [".", ".."] {
            let entry = CatalogEntry {
                name: name.into(),
                ..valid_entry()
            };
            let err = validate_entry(&entry, Path::new("plugins.toml")).unwrap_err();
            assert!(matches!(err, InstallError::Catalog { .. }));
        }
    }

    #[test]
    fn catalog_rejects_a_manifest_with_an_unsupported_api_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        let mut manifest = valid_manifest();
        manifest.api_version = pwr_plugin_protocol::API_VERSION + 1;
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"https://example.com/hello\"\n",
                    "sha256 = \"{}\"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                serde_json::to_string(&manifest).unwrap()
            ),
        )
        .unwrap();
        let err = PluginCatalog::load(&path).unwrap_err();
        assert!(matches!(err, InstallError::Catalog { .. }));
    }

    #[test]
    fn catalog_rejects_a_manifest_naming_a_different_plugin() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("plugins.toml");
        let mut manifest = valid_manifest();
        manifest.name = "other".into();
        fs::write(
            &path,
            format!(
                concat!(
                    "[[plugins]]\nname = \"hello\"\nurl = \"https://example.com/hello\"\n",
                    "sha256 = \"{}\"\nmanifest = '{}'\n",
                ),
                "ab".repeat(32),
                serde_json::to_string(&manifest).unwrap()
            ),
        )
        .unwrap();
        let err = PluginCatalog::load(&path).unwrap_err();
        assert!(matches!(err, InstallError::Catalog { .. }));
    }
}
