use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, Request, State},
    response::Response,
};
use http::header::HOST;

use crate::{
    ServerState,
    error::Error,
    utils::{HopByHopHeadersExt, IntoReqwestRequest},
};

pub async fn handle_image_request(
    State(state): State<Arc<ServerState>>,
    Path(request_hash): Path<u64>,
    incoming_request: Request<Body>,
) -> Result<Response, Error> {
    let mut stored_request = state
        .image_requests
        .remove(&request_hash)
        .await
        .ok_or(Error::NotFound)?;

    for (name, value) in incoming_request.headers().iter() {
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
    if !status.is_success() {
        return Err(Error::RemoteServer(status));
    }

    let mut headers = response.headers().clone();
    headers.remove_hop_by_hop_headers();

    let stream = response.bytes_stream();
    let body = Body::from_stream(stream);

    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;

    Ok(response)
}
