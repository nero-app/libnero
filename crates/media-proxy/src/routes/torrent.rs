use std::sync::Arc;
#[cfg(feature = "torrent-librqbit")]
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, State},
    response::Response,
};
use http::{Request, StatusCode, header::CONTENT_TYPE};

use crate::{
    ServerState,
    error::Error,
    torrent::{AddTorrentOptions, Torrent, TorrentSource},
};

#[tracing::instrument(level = "debug", skip(state, source))]
pub(super) async fn handle_torrent_resource(
    state: Arc<ServerState>,
    resource_id: String,
    source: TorrentSource,
) -> Result<Response, Error> {
    let backend = &state.torrent_backend;

    let torrent = if let Some(torrent) = state
        .active_torrents
        .read()
        .await
        .get(&resource_id)
        .cloned()
    {
        tracing::debug!(torrent_id = %torrent.id, "reusing active torrent");
        torrent
    } else {
        let options = if let Some(selector) = &state.torrent_file_selector {
            let files = backend.list_files(&source).await?;
            tracing::debug!(count = files.len(), "selecting torrent files");
            let file_indices = selector.select(&files).await?;
            tracing::trace!(?file_indices, "selected torrent files");
            Some(AddTorrentOptions { file_indices })
        } else {
            None
        };

        tracing::debug!("adding torrent");
        let torrent = backend.add_torrent(source, options).await?;
        tracing::debug!(torrent_id = %torrent.id, files = torrent.files.len(), "added torrent");
        state
            .active_torrents
            .write()
            .await
            .insert(resource_id.clone(), torrent.clone());
        torrent
    };

    Ok(playlist(&resource_id, torrent))
}

fn playlist(resource_id: &str, torrent: Torrent) -> Response {
    let mut m3u = String::from("#EXTM3U\n");
    for file in torrent.files {
        let url = format!("{resource_id}/stream/{}", file.index);

        m3u.push_str(&format!("#EXTINF:-1,{}\n{}\n", file.name, url));
    }

    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/x-mpegurl")
        .body(Body::new(m3u))
        .unwrap()
}

#[tracing::instrument(
    level = "debug", skip_all,
    fields(%resource_id, file_index, torrent_id, range = ?incoming_request.headers().get(http::header::RANGE)),
    err(level = "warn")
)]
pub(crate) async fn handle_torrent_stream_request(
    State(state): State<Arc<ServerState>>,
    Path((resource_id, file_index)): Path<(String, usize)>,
    incoming_request: axum::extract::Request,
) -> Result<Response, Error> {
    tracing::debug!("serving torrent stream");
    let backend = &state.torrent_backend;

    let torrent_id = state
        .active_torrents
        .read()
        .await
        .get(&resource_id)
        .map(|torrent| torrent.id.clone())
        .ok_or(Error::NotFound)?;
    tracing::Span::current().record("torrent_id", &torrent_id);

    let (parts, _body) = incoming_request.into_parts();

    #[cfg(not(feature = "torrent-librqbit"))]
    return backend
        .handle_stream_request(
            &torrent_id,
            file_index,
            Request::from_parts(parts, Body::empty()),
        )
        .await
        .inspect(|response| tracing::trace!(status = response.status().as_u16(), "stream response"))
        .map_err(Into::into);

    #[cfg(feature = "torrent-librqbit")]
    let mut waiting_for_metadata = false;

    #[cfg(feature = "torrent-librqbit")]
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
            Ok(resp) => {
                tracing::trace!(status = resp.status().as_u16(), "stream response");
                return Ok(resp);
            }
            #[cfg(feature = "torrent-librqbit")]
            Err(err)
                if err.to_string().contains("initializing")
                    || err.to_string().contains("metadata") =>
            {
                if !waiting_for_metadata {
                    tracing::debug!("waiting for torrent metadata");
                    waiting_for_metadata = true;
                }
                tracing::trace!("torrent is not ready; retrying in 200 ms");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            Err(err) => return Err(err.into()),
        }
    }
}
