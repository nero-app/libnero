use mime::Mime;
use reqwest::Client;
use std::str::FromStr;
use tracing::{debug, warn};

use crate::HttpRequest;

pub async fn mime_type(
    client: &Client,
    request: &HttpRequest,
) -> Result<Option<Mime>, reqwest::Error> {
    if let Some(mime) = detect_from_path(request) {
        debug!("MIME type detected from URL path: {}", mime);
        return Ok(Some(mime));
    }

    if let Some(mime) = detect_from_head(client, request).await? {
        debug!("MIME type detected from HEAD request: {}", mime);
        return Ok(Some(mime));
    }

    if let Some(mime) = detect_from_content(client, request).await? {
        debug!("MIME type detected from content: {}", mime);
        return Ok(Some(mime));
    }

    warn!("Could not detect MIME type for URL: {}", request.uri());
    Ok(None)
}

fn detect_from_path(request: &HttpRequest) -> Option<Mime> {
    let path = request.uri().path();
    let extension = path.rsplit('.').next()?;

    if extension == path || extension.contains('/') {
        return None;
    }

    let mime = mime_guess::from_ext(extension).first()?;
    Some(mime)
}

async fn detect_from_head(
    client: &Client,
    request: &HttpRequest,
) -> Result<Option<Mime>, reqwest::Error> {
    let res = client.head(request.uri().to_string()).send().await?;

    if !res.status().is_success() {
        debug!("HEAD request failed with status: {}", res.status());
        return Ok(None);
    }

    let content_type = res
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());

    if let Some(content_type) = content_type {
        let mime = Mime::from_str(content_type).ok();
        debug!("Content-Type from HEAD: {:?}", mime);
        return Ok(mime);
    }

    Ok(None)
}

async fn detect_from_content(
    client: &Client,
    request: &HttpRequest,
) -> Result<Option<Mime>, reqwest::Error> {
    let mut req = client
        .request(request.method().clone(), request.uri().to_string())
        .headers(request.headers().clone());

    if let Some(body) = request.body() {
        req = req.body(body.clone());
    }

    let mut res = client.execute(req.build()?).await?;

    if !res.status().is_success() {
        debug!("Content request failed with status: {}", res.status());
        return Ok(None);
    }

    let Some(chunk) = res.chunk().await? else {
        return Ok(None);
    };

    let mime = infer::get(&chunk).and_then(|kind| Mime::from_str(kind.mime_type()).ok());

    Ok(mime)
}
