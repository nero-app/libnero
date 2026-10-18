use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    response::Response,
};
use http::header::HOST;

use crate::{
    ServerState,
    error::Error,
    resources::Resource,
    utils::{HopByHopHeadersExt, IntoReqwestRequest},
};

pub(crate) async fn handle_media_request(
    State(state): State<Arc<ServerState>>,
    Path(resource_id): Path<String>,
    incoming_request: axum::extract::Request,
) -> Result<Response, Error> {
    let resource = state
        .resources
        .read()
        .await
        .get(&resource_id)
        .cloned()
        .ok_or(Error::NotFound)?;

    match resource {
        Resource::Http(stored_request) => {
            handle_http_request(state, stored_request, incoming_request).await
        }
        #[cfg(feature = "torrent")]
        Resource::Torrent(source) => {
            super::torrent::handle_torrent_resource(state, resource_id, source).await
        }
    }
}

async fn handle_http_request(
    state: Arc<ServerState>,
    mut stored_request: Box<crate::HttpRequest>,
    incoming_request: axum::extract::Request,
) -> Result<Response, Error> {
    for (name, value) in incoming_request.headers() {
        if name == HOST {
            continue;
        }
        stored_request
            .headers_mut()
            .insert(name.clone(), value.clone());
    }

    stored_request.headers_mut().remove_hop_by_hop_headers();

    let request = stored_request.into_reqwest_request(state.http_client.clone())?;
    let response = state.http_client.execute(request).await?;

    let status = response.status();
    let mut headers = response.headers().clone();
    headers.remove_hop_by_hop_headers();

    let mut response = Response::new(Body::from_stream(response.bytes_stream()));
    *response.status_mut() = status;
    *response.headers_mut() = headers;

    Ok(response)
}
