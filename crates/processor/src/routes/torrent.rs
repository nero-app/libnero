use std::sync::Arc;

use axum::{
    extract::{Path, State},
    response::Response,
};

use crate::{ServerState, error::Error};

pub async fn handle_torrent_request(
    State(state): State<Arc<ServerState>>,
    Path(request_hash): Path<u64>,
) -> Result<Response, Error> {
    let stored_request = state
        .video_requests
        .get(&request_hash)
        .await
        .ok_or(Error::NotFound)?;

    state
        .current_video
        .write()
        .await
        .replace(stored_request.clone());

    let api_guard = state.torrent_api.read().await;
    let torrent_api = api_guard.as_ref().ok_or(Error::TorrentSupportDisabled)?;

    todo!()
}
