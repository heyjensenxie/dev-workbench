use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("process operation failed: {0}")]
    Process(#[from] workbench_process::ProcessError),
    #[error("network operation failed: {0}")]
    Network(#[from] workbench_network::NetworkError),
    #[error("API request failed: {0}")]
    Api(#[from] crate::api::ApiError),
    /// Vault failures keep their own wording: "Incorrect master password." is the
    /// single, deliberately uniform message and must not be rephrased here.
    #[error("{0}")]
    Vault(#[from] workbench_vault::VaultError),
    #[error("invalid input: {0}")]
    Validation(String),
    #[error("resource not found: {0}")]
    NotFound(String),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorPayload<'a> {
    code: &'a str,
    message: String,
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let code = match self {
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Vault(workbench_vault::VaultError::Validation(_)) => "VALIDATION_ERROR",
            // A locked vault and a wrong master password both need their own
            // handling in the UI, so they are surfaced as distinct codes without
            // ever expanding the message.
            Self::Vault(workbench_vault::VaultError::Locked) => "VAULT_LOCKED",
            Self::Vault(workbench_vault::VaultError::LockedOut { .. }) => "VAULT_LOCKED_OUT",
            Self::Vault(workbench_vault::VaultError::IncorrectMasterPassword) => {
                "VAULT_BAD_PASSWORD"
            }
            Self::Vault(_) => "VAULT_ERROR",
            _ => "NATIVE_ERROR",
        };
        ErrorPayload {
            code,
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}
