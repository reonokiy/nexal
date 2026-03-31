use chrono::DateTime;
use chrono::Utc;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use std::fmt::Debug;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use nexal_app_server_protocol::AuthMode;

/// Determine where Nexal should store CLI auth credentials.
///
/// Only file-based storage is supported. Legacy values ("keyring", "auto",
/// "ephemeral") are accepted during deserialization for backward compatibility
/// but silently map to `File`.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, JsonSchema)]
pub enum AuthCredentialsStoreMode {
    #[default]
    /// Persist credentials in NEXAL_HOME/auth.json.
    File,
}

impl Serialize for AuthCredentialsStoreMode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("file")
    }
}

impl<'de> Deserialize<'de> for AuthCredentialsStoreMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        // Accept legacy values; all map to File.
        match s.as_str() {
            "file" | "keyring" | "auto" | "ephemeral" => Ok(Self::File),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["file"],
            )),
        }
    }
}

/// Expected structure for $NEXAL_HOME/auth.json.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct AuthDotJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<AuthMode>,

    #[serde(rename = "OPENAI_API_KEY")]
    pub openai_api_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<DateTime<Utc>>,
}

pub(super) fn get_auth_file(nexal_home: &Path) -> PathBuf {
    nexal_home.join("auth.json")
}

pub(super) fn delete_file_if_exists(nexal_home: &Path) -> std::io::Result<bool> {
    let auth_file = get_auth_file(nexal_home);
    match std::fs::remove_file(&auth_file) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) trait AuthStorageBackend: Debug + Send + Sync {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>>;
    fn save(&self, auth: &AuthDotJson) -> std::io::Result<()>;
    fn delete(&self) -> std::io::Result<bool>;
}

#[derive(Clone, Debug)]
pub(super) struct FileAuthStorage {
    nexal_home: PathBuf,
}

impl FileAuthStorage {
    pub(super) fn new(nexal_home: PathBuf) -> Self {
        Self { nexal_home }
    }

    /// Attempt to read and parse the `auth.json` file in the given `NEXAL_HOME` directory.
    /// Returns the full AuthDotJson structure.
    pub(super) fn try_read_auth_json(&self, auth_file: &Path) -> std::io::Result<AuthDotJson> {
        let mut file = File::open(auth_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let auth_dot_json: AuthDotJson = serde_json::from_str(&contents)?;

        Ok(auth_dot_json)
    }
}

impl AuthStorageBackend for FileAuthStorage {
    fn load(&self) -> std::io::Result<Option<AuthDotJson>> {
        let auth_file = get_auth_file(&self.nexal_home);
        let auth_dot_json = match self.try_read_auth_json(&auth_file) {
            Ok(auth) => auth,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err),
        };
        Ok(Some(auth_dot_json))
    }

    fn save(&self, auth_dot_json: &AuthDotJson) -> std::io::Result<()> {
        let auth_file = get_auth_file(&self.nexal_home);

        if let Some(parent) = auth_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json_data = serde_json::to_string_pretty(auth_dot_json)?;
        let mut options = OpenOptions::new();
        options.truncate(true).write(true).create(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(auth_file)?;
        file.write_all(json_data.as_bytes())?;
        file.flush()?;
        Ok(())
    }

    fn delete(&self) -> std::io::Result<bool> {
        delete_file_if_exists(&self.nexal_home)
    }
}

pub(super) fn create_auth_storage(
    nexal_home: PathBuf,
    _mode: AuthCredentialsStoreMode,
) -> Arc<dyn AuthStorageBackend> {
    Arc::new(FileAuthStorage::new(nexal_home))
}
