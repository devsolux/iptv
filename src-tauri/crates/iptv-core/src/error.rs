use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

/// Public errors deliberately do not expose provider URLs, headers, paths or SQL.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Unlock the local vault first.")]
    Locked,
    #[error("The passphrase is incorrect or the vault is damaged.")]
    Vault,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("The source is not available. Check its URL, connection and approved CDN hosts.")]
    Network,
    #[error("This network address or redirect is not permitted by the source policy.")]
    Policy,
    #[error("The configured resource limit was exceeded.")]
    Limit,
    #[error("The operation was cancelled.")]
    Cancelled,
    #[error("Another import is already active for this source.")]
    Busy,
    #[error("The catalogue changed. Reload this window of results.")]
    Stale,
    #[error("The requested item is no longer available.")]
    NotFound,
    #[error("This stream format is not supported by this player.")]
    Unsupported,
    #[error("Local storage could not complete the operation.")]
    Storage,
    #[error("The native player could not start.")]
    Player,
    #[error(
        "The stream did not respond or refused the request. It may be offline, geo-blocked or need another variant."
    )]
    Stream,
    #[error(
        "A stream address was blocked by this source's network policy (unencrypted HTTP, an unverified certificate, a private address or an unsupported redirect). Adjust the source's permissions if its streams need them."
    )]
    StreamPolicy,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicError {
    pub code: &'static str,
    pub message: String,
}
impl Error {
    pub fn public(&self) -> PublicError {
        let code = match self {
            Self::Locked => "LOCKED",
            Self::Vault => "VAULT",
            Self::Invalid(_) => "INVALID",
            Self::Network => "NETWORK",
            Self::Policy => "POLICY",
            Self::Limit => "LIMIT",
            Self::Cancelled => "CANCELLED",
            Self::Busy => "BUSY",
            Self::Stale => "STALE",
            Self::NotFound => "NOT_FOUND",
            Self::Unsupported => "UNSUPPORTED",
            Self::Storage => "STORAGE",
            Self::Player => "PLAYER",
            Self::Stream => "STREAM",
            Self::StreamPolicy => "STREAM_POLICY",
        };
        PublicError {
            code,
            message: self.to_string(),
        }
    }
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self::Invalid("Invalid structured data.")
    }
}
