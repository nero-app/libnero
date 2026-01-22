mod image;
#[cfg(feature = "torrent")]
mod torrent;
mod video;

pub use image::*;
#[cfg(feature = "torrent")]
pub use torrent::*;
pub use video::*;

use bytes::Bytes;
use http::{
    HeaderMap, HeaderName,
    header::{CONNECTION, PROXY_AUTHENTICATE, PROXY_AUTHORIZATION, TE, TRANSFER_ENCODING, UPGRADE},
};
use reqwest::Client;
use url::Url;

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
