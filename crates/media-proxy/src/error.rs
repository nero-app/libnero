use axum::response::{IntoResponse, Response};
use http::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error("Request not found")]
    NotFound,

    #[error("Reqwest HTTP error: {0}")]
    Reqwest(#[from] reqwest::Error),

    #[cfg(feature = "torrent")]
    #[error("Torrent error: {0}")]
    TorrentBackend(#[from] anyhow::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Reqwest(_) => StatusCode::BAD_GATEWAY,
            #[cfg(feature = "torrent")]
            Error::TorrentBackend(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        (status, self.to_string()).into_response()
    }
}
