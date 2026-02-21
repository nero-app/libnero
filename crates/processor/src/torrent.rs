use std::path::PathBuf;
#[cfg(feature = "torrent-librqbit")]
use std::sync::Arc;

use anyhow::Result;
use http::{Request, Response};

use crate::TorrentSource;
#[cfg(feature = "torrent-librqbit")]
use crate::cache::Cache;

#[derive(Clone, Debug)]
pub struct AddTorrentOptions {
    pub file_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct Torrent {
    pub id: String,
    pub name: Option<String>,
    pub files: Vec<TorrentFile>,
}

#[derive(Debug, Clone)]
pub struct TorrentFile {
    pub index: usize,
    pub name: String,
    pub path: PathBuf,
}

#[async_trait::async_trait]
pub trait TorrentBackend: Send + Sync {
    async fn list_files(&self, source: &TorrentSource) -> Result<Vec<TorrentFile>>;

    async fn add_torrent(
        &self,
        source: TorrentSource,
        options: Option<AddTorrentOptions>,
    ) -> Result<Torrent>;

    async fn handle_stream_request(
        &self,
        torrent_id: &str,
        file_index: usize,
        request: Request<axum::body::Body>,
    ) -> Result<Response<axum::body::Body>>;

    async fn cancel_torrent(&self, torrent: &str) -> Result<()>;
}

#[cfg(feature = "torrent-librqbit")]
pub struct RqbitTorrentBackend {
    api: librqbit::Api,
    client: reqwest::Client,
    files_cache: Cache<u64, Vec<TorrentFile>>,
}

#[cfg(feature = "torrent-librqbit")]
impl RqbitTorrentBackend {
    pub fn new(session: Arc<librqbit::Session>, client: reqwest::Client) -> Self {
        Self {
            api: librqbit::Api::new(session, None),
            client,
            files_cache: Cache::default(),
        }
    }

    async fn resolve_torrent_source(
        &self,
        source: TorrentSource,
    ) -> Result<librqbit::AddTorrent<'static>> {
        match source {
            TorrentSource::Http(mut request) => {
                use crate::utils::{HopByHopHeadersExt, IntoReqwestRequest};

                request.headers_mut().remove_hop_by_hop_headers();
                let req = request.into_reqwest_request(self.client.clone())?;

                let bytes = self.client.execute(req).await?.bytes().await?;
                Ok(librqbit::AddTorrent::from_bytes(bytes.to_vec()))
            }
            TorrentSource::MagnetUri(uri) => Ok(librqbit::AddTorrent::from_url(uri)),
        }
    }
}

#[cfg(feature = "torrent-librqbit")]
#[async_trait::async_trait]
impl TorrentBackend for RqbitTorrentBackend {
    async fn list_files(&self, source: &TorrentSource) -> Result<Vec<TorrentFile>> {
        use librqbit::{AddTorrent, AddTorrentOptions};

        use crate::utils::get_torrent_source_hash;

        let request_hash = get_torrent_source_hash(source);
        if let Some(files) = self.files_cache.get(&request_hash).await {
            return Ok(files.clone());
        }

        let uri = match source {
            TorrentSource::Http(request) => &request.uri().to_string(),
            TorrentSource::MagnetUri(uri) => uri,
        };

        let options = AddTorrentOptions {
            overwrite: true,
            list_only: true,
            ..Default::default()
        };
        let response = self
            .api
            .api_add_torrent(AddTorrent::from_url(uri), Some(options))
            .await?;

        let files = response
            .details
            .files
            .ok_or(anyhow::anyhow!("Torrent has no files"))?
            .into_iter()
            .enumerate()
            .filter_map(|(index, f)| {
                let path = PathBuf::from(&f.name);
                let name = path.file_name()?.to_string_lossy().to_string();

                Some(TorrentFile { index, name, path })
            })
            .collect::<Vec<_>>();

        if files.is_empty() {
            anyhow::bail!("No valid files found in torrent")
        }

        self.files_cache.insert(request_hash, files.clone()).await;

        Ok(files)
    }

    async fn add_torrent(
        &self,
        source: TorrentSource,
        options: Option<AddTorrentOptions>,
    ) -> Result<Torrent> {
        use librqbit::AddTorrentOptions;

        let add_torrent = self.resolve_torrent_source(source).await?;

        let options = match options {
            Some(options) => Some(AddTorrentOptions {
                only_files: Some(options.file_indices),
                overwrite: true,
                ..Default::default()
            }),
            None => None,
        };

        let added = self.api.api_add_torrent(add_torrent, options).await?;

        let files = added
            .details
            .files
            .ok_or(anyhow::anyhow!("Torrent has no files"))?
            .into_iter()
            .enumerate()
            .filter(|(_, f)| f.included)
            .filter_map(|(index, f)| {
                let path = PathBuf::from(f.name);
                let name = path.file_name()?.to_string_lossy().to_string();

                Some(TorrentFile { index, name, path })
            })
            .collect::<Vec<_>>();

        if files.is_empty() {
            return Err(anyhow::anyhow!("No valid files were included in torrent"));
        }

        Ok(Torrent {
            id: added
                .id
                .ok_or(anyhow::anyhow!("Torrent ID not available"))?
                .to_string(),
            name: added.details.name,
            files,
        })
    }

    async fn handle_stream_request(
        &self,
        torrent_id: &str,
        file_index: usize,
        request: Request<axum::body::Body>,
    ) -> Result<Response<axum::body::Body>> {
        use http::{HeaderMap, StatusCode};
        use librqbit::api::TorrentIdOrHash;
        use std::io::SeekFrom;
        use tokio::io::{AsyncReadExt, AsyncSeekExt};

        let headers = request.headers();

        let torrent_id = TorrentIdOrHash::Id(torrent_id.parse()?);

        let mut stream = self.api.api_stream(torrent_id, file_index)?;

        let total_len = stream.len();
        let mut status = StatusCode::OK;
        let mut response_headers = HeaderMap::new();

        response_headers.insert(
            http::header::ACCEPT_RANGES,
            http::HeaderValue::from_static("bytes"),
        );

        let range = headers
            .get(http::header::RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("bytes="))
            .and_then(|v| v.split_once('-'))
            .and_then(|(start, end)| {
                let start = start.parse::<u64>().ok()?;
                let end = end.parse::<u64>().ok().map(|v| v + 1);
                Some((start, end))
            });

        let reader: Box<dyn tokio::io::AsyncRead + Send + Unpin> = if let Some((start, end)) = range
        {
            status = StatusCode::PARTIAL_CONTENT;

            stream.seek(SeekFrom::Start(start)).await?;

            let end = end.unwrap_or(total_len);
            let len = end - start;

            response_headers.insert(
                http::header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end - 1, total_len)
                    .parse()
                    .unwrap(),
            );

            response_headers.insert(
                http::header::CONTENT_LENGTH,
                len.to_string().parse().unwrap(),
            );

            Box::new(stream.take(len))
        } else {
            response_headers.insert(
                http::header::CONTENT_LENGTH,
                total_len.to_string().parse().unwrap(),
            );

            Box::new(stream)
        };

        let body = axum::body::Body::from_stream(tokio_util::io::ReaderStream::with_capacity(
            reader,
            64 * 1024,
        ));

        let mut builder = Response::builder().status(status);

        for (key, value) in response_headers.iter() {
            builder = builder.header(key, value);
        }

        Ok(builder.body(body).unwrap())
    }

    async fn cancel_torrent(&self, torrent: &str) -> Result<()> {
        use librqbit::api::TorrentIdOrHash;

        let idx = TorrentIdOrHash::Id(torrent.parse()?);
        self.api.api_torrent_action_delete(idx).await?;

        Ok(())
    }
}
