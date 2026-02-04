use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use bytes::Bytes;
use http::Request;

use crate::TorrentSource;

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
