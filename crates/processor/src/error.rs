use axum::response::{IntoResponse, Response};
use http::StatusCode;
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Request not found")]
    NotFound,

    #[error("Reqwest HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[error("Remote server returned status {0}")]
    RemoteServer(StatusCode),

    #[cfg(feature = "torrent")]
    #[error("Torrent support is disabled")]
    TorrentSupportDisabled,

    #[cfg(feature = "torrent")]
    #[error("Torrent error: {0}")]
    TorrentBackend(#[from] anyhow::Error),

    #[cfg(feature = "torrent")]
    #[error("Invalid request type")]
    InvalidRequestType,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Reqwest(_) => {
                error!("Reqwest error: {:#}", self);
                StatusCode::BAD_GATEWAY
            }
            Error::RemoteServer(code) => {
                error!("Remote server returned status {}: {:#}", code, self);
                StatusCode::BAD_GATEWAY
            }
            #[cfg(feature = "torrent")]
            Error::TorrentSupportDisabled => {
                error!("Torrent support is disabled: {:#}", self);
                StatusCode::BAD_REQUEST
            }
            #[cfg(feature = "torrent")]
            Error::TorrentBackend(e) => {
                error!("Torrent backend error: {:#}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            }
            #[cfg(feature = "torrent")]
            Error::InvalidRequestType => {
                error!("Invalid request type: {:#}", self);
                StatusCode::BAD_REQUEST
            }
        };

        (status, self.to_string()).into_response()
    }
}
