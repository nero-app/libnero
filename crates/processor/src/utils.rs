use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytes::Bytes;
use http::Request;
use http::{
    HeaderMap, HeaderName,
    header::{CONNECTION, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRANSFER_ENCODING, UPGRADE},
};
use reqwest::Client;
use url::Url;

#[cfg(feature = "torrent")]
use crate::TorrentSource;

const HOP_BY_HOP_HEADERS: [HeaderName; 8] = [
    CONNECTION,
    HeaderName::from_static("keep-alive"),
    PROXY_AUTHENTICATE,
    PROXY_AUTHORIZATION,
    TE,
    HeaderName::from_static("trailers"),
    TRANSFER_ENCODING,
    UPGRADE,
];

pub trait HopByHopHeadersExt {
    fn remove_hop_by_hop_headers(&mut self);
}

impl HopByHopHeadersExt for HeaderMap {
    fn remove_hop_by_hop_headers(&mut self) {
        let connection_val_owned = self
            .get(CONNECTION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_owned());

        if let Some(conn_str) = connection_val_owned {
            for token in conn_str.split(',') {
                let token = token.trim();
                if !token.is_empty()
                    && let Ok(header_name) = HeaderName::from_bytes(token.as_bytes())
                {
                    self.remove(header_name);
                }
            }
        }

        for header in HOP_BY_HOP_HEADERS {
            self.remove(header);
        }
    }
}

pub trait IntoReqwestRequest {
    fn into_reqwest_request(self, client: Client) -> Result<reqwest::Request, reqwest::Error>;
}

impl IntoReqwestRequest for http::Request<Option<Bytes>> {
    fn into_reqwest_request(self, client: Client) -> Result<reqwest::Request, reqwest::Error> {
        let (parts, body) = self.into_parts();

        let url = Url::parse(&parts.uri.to_string()).unwrap();

        let mut builder = client.request(parts.method, url);

        for (k, v) in parts.headers.iter() {
            builder = builder.header(k, v);
        }

        if let Some(body) = body {
            builder.body(body).build()
        } else {
            builder.build()
        }
    }
}

#[cfg(feature = "torrent")]
pub fn get_torrent_source_hash(source: &TorrentSource) -> u64 {
    let mut hasher = DefaultHasher::new();

    match source {
        TorrentSource::Http(req) => {
            0u8.hash(&mut hasher);
            get_request_hash(req).hash(&mut hasher);
        }
        TorrentSource::MagnetUri(uri) => {
            1u8.hash(&mut hasher);
            uri.hash(&mut hasher);
        }
    }

    hasher.finish()
}

pub fn get_request_hash(request: &Request<Option<Bytes>>) -> u64 {
    let mut hasher = DefaultHasher::new();

    request.uri().hash(&mut hasher);

    request.method().hash(&mut hasher);

    let mut headers = request
        .headers()
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_bytes()))
        .collect::<Vec<_>>();
    headers.sort_unstable_by_key(|(k, _)| *k);

    for (name, value) in headers {
        name.hash(&mut hasher);
        value.hash(&mut hasher);
    }

    if let Some(body) = request.body() {
        body.hash(&mut hasher);
    }

    hasher.finish()
}
