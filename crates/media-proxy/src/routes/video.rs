use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path, State},
    response::Response,
};

use crate::{
    ServerState,
    error::Error,
    resources::Resource,
    utils::{HopByHopHeadersExt, IntoReqwestRequest},
};

pub async fn handle_video_request(
    State(state): State<Arc<ServerState>>,
    Path(resource_id): Path<String>,
    incoming_request: axum::extract::Request,
) -> Result<Response, Error> {
    let resource = state
        .resource_store
        .remove(&resource_id)
        .await
        .ok_or(Error::NotFound)?;

    #[allow(irrefutable_let_patterns)]
    let Resource::Http(mut stored_request) = resource else {
        return Err(Error::InvalidResourceKind);
    };

    state
        .current_video
        .write()
        .await
        .replace(Resource::Http(stored_request.clone()));

    for (name, value) in incoming_request.headers().iter() {
        if name == http::header::HOST {
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
