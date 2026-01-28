use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{Path, State},
    response::Response,
};
use http::{Request, StatusCode, header::CONTENT_TYPE, uri::Scheme};

use crate::{CurrentVideo, ServerState, error::Error, torrent::AddTorrentOptions};

pub async fn handle_torrent_request(
    State(state): State<Arc<ServerState>>,
    Path(request_hash): Path<u64>,
) -> Result<Response, Error> {
    let stored_request = state
        .video_requests
        .remove(&request_hash)
        .await
        .ok_or(Error::NotFound)?;

    let backend_guard = state.torrent_backend.read().await;
    let backend = backend_guard
        .as_ref()
        .ok_or(Error::TorrentSupportDisabled)?;

    let crate::Request::Torrent {
        source,
        file_indices,
    } = stored_request
    else {
        return Err(Error::InvalidRequestType);
    };

    {
        let mut current = state.current_video.write().await;
        if let Some(CurrentVideo::Torrent { torrent_id }) = current.take() {
            backend.cancel_torrent(&torrent_id).await.ok();
        }
    }

    let added = backend
        .add_torrent(source, Some(AddTorrentOptions { file_indices }))
        .await?;

    {
        let mut current = state.current_video.write().await;
        *current = Some(CurrentVideo::Torrent {
            torrent_id: added.id.clone(),
        });
    }

    let mut m3u = String::from("#EXTM3U\n");
    for file in added.files {
        let url = format!(
            "{}://{}/torrent/{}/stream/{}",
            Scheme::HTTP,
            state.addr,
            added.id,
            file.index
        );

        m3u.push_str(&format!("#EXTINF:-1,{}\n{}\n", file.name, url));
    }

    let response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-mpegurl")
        .body(Body::new(m3u))
        .unwrap();

    Ok(response)
}

pub async fn handle_torrent_stream_request(
    State(state): State<Arc<ServerState>>,
    Path((torrent_id, file_index)): Path<(String, usize)>,
    incoming_request: axum::extract::Request,
) -> Result<Response, Error> {
    let backend_guard = state.torrent_backend.read().await;
    let backend = backend_guard
        .as_ref()
        .ok_or(Error::TorrentSupportDisabled)?;

    let (parts, _body) = incoming_request.into_parts();

    // TODO: any better way to do this?
    loop {
        match backend
            .handle_stream_request(
                &torrent_id,
                file_index,
                Request::from_parts(parts.clone(), Body::empty()),
            )
            .await
        {
            Ok(resp) => return Ok(resp),
            #[cfg(feature = "torrent-librqbit")]
            Err(err)
                if err.to_string().contains("initializing")
                    || err.to_string().contains("metadata") =>
            {
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }
}
