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

    #[error("Torrent support is disabled")]
    TorrentSupportDisabled,

    #[error("Torrent API error: {0}")]
    TorrentApi(#[from] librqbit::ApiError),
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
            Error::TorrentSupportDisabled => {
                error!("Torrent support is disabled: {:#}", self);
                StatusCode::BAD_REQUEST
            }
            Error::TorrentApi(e) => {
                error!("Torrent API error: {:#}", e);
                StatusCode::BAD_REQUEST
            }
        };

        (status, self.to_string()).into_response()
    }
}
