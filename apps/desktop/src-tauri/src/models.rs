use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::BTreeMap;

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[derive(Clone, Debug, Deserialize, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[sqlx(rename = "created_at")]
    pub created_at: i64,
    #[sqlx(rename = "updated_at")]
    pub updated_at: i64,
    #[sqlx(rename = "last_opened_at")]
    pub last_opened_at: Option<i64>,
}

/// A recognised file in the project, relative to the project root.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedFile {
    pub path: String,
    pub kind: String,
}

/// A runnable script declared by a manifest.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedScript {
    pub name: String,
    pub command: String,
    pub source: String,
}

/// A service the scanner believes the project can start.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedService {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub port: Option<u16>,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VcsInfo {
    pub kind: String,
    pub branch: Option<String>,
}

impl VcsInfo {
    pub fn git(branch: Option<String>) -> Self {
        Self {
            kind: "git".into(),
            branch,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContext {
    pub id: String,
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub package_managers: Vec<String>,
    pub detected_files: Vec<DetectedFile>,
    pub scripts: Vec<DetectedScript>,
    pub suggested_services: Vec<SuggestedService>,
    pub compose_services: Vec<String>,
    pub monorepo: bool,
    pub vcs: Option<VcsInfo>,
    pub scanned_at: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DevService {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    pub port: Option<u16>,
    #[serde(default)]
    pub auto_open: bool,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub updated_at: i64,
}

impl DevService {
    /// Rejects services that could never be started or persisted.
    pub fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("service name must not be empty".into());
        }
        if name.chars().count() > 80 {
            return Err("service name must be 80 characters or fewer".into());
        }
        if self.command.trim().is_empty() {
            return Err("service command must not be empty".into());
        }
        if self.project_id.trim().is_empty() {
            return Err("service must belong to a project".into());
        }
        if self.id.trim().is_empty() {
            return Err("service id must not be empty".into());
        }
        if self.dependencies.iter().any(|id| id == &self.id) {
            return Err("service cannot depend on itself".into());
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceState {
    pub service_id: String,
    pub status: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningService {
    pub service_id: String,
    pub pid: u32,
}

#[derive(Clone, Debug, Deserialize, FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiModule {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A saved API request template. Secret values are scrubbed before persistence.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedApiRequest {
    pub id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub module_id: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
