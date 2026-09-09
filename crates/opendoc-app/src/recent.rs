use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AppRecentDocument {
    pub uuid: String,
    pub title: String,
    pub doi: Option<String>,
    pub repository_root: String,
    pub repository_backend: String,
    pub repository_namespace: Option<String>,
    pub last_manifest: Option<String>,
    pub updated_at_ms: u64,
}
