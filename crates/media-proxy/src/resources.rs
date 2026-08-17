use serde::{Deserialize, Serialize};
use url::Url;

use crate::HttpRequest;
#[cfg(feature = "torrent")]
use crate::torrent::TorrentSource;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MediaReference(String);

impl MediaReference {
    pub(crate) fn new(id: String) -> Self {
        Self(id)
    }

    pub fn id(&self) -> &str {
        &self.0
    }

    pub fn resolve(&self, proxy_base: &Url) -> Result<Url, url::ParseError> {
        proxy_base.join(&format!("media/{}", self.0))
    }
}

#[derive(Debug, Clone)]
pub enum Resource {
    Http(Box<HttpRequest>),
    #[cfg(feature = "torrent")]
    Torrent(TorrentSource),
}
