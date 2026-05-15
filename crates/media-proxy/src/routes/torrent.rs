use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{Path, State},
    response::Response,
};
use http::{Request, StatusCode, header::CONTENT_TYPE, uri::Scheme};

use crate::{ServerState, error::Error, resources::ResourceData, torrent::AddTorrentOptions};

pub async fn handle_torrent_request(
    State(state): State<Arc<ServerState>>,
    Path(resource_id): Path<String>,
) -> Result<Response, Error> {
    let resource = state
        .resource_store
        .remove(&resource_id)
        .await
        .ok_or(Error::NotFound)?;

    let backend = state
        .torrent_backend
        .as_ref()
        .ok_or(Error::TorrentSupportDisabled)?;

    let ResourceData::Torrent(source) = resource.data else {
        return Err(Error::InvalidResourceKind);
    };

    {
        let mut current = state.current_torrent.write().await;
        if let Some(torrent) = current.take() {
            backend.cancel_torrent(&torrent.id).await.ok();
        }
    }

    let options = if let Some(selector) = &state.torrent_file_selector {
        let files = backend.list_files(&source).await?;
        let file_indices = selector.select(&files).await?;
        Some(AddTorrentOptions { file_indices })
    } else {
        None
    };

    let added = backend.add_torrent(source, options).await?;

    {
        let mut current = state.current_torrent.write().await;
        *current = Some(added.clone());
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
    let backend = state
        .torrent_backend
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
