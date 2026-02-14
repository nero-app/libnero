use std::str::FromStr;

use self::nero::extension::types::{
    Episode, EpisodesPage, Filter, FilterCategory, SearchFilter, Series, SeriesPage, Video,
};

use anyhow::Result;
use magnet_uri::MagnetURI;
use nero_keyvalue_ttl::KeyValueTTL;
use semver::Version;
use wasmtime::{
    Engine,
    component::{Linker, Resource, bindgen},
};
use wasmtime_wasi_http::{WasiHttpView, types::HostOutgoingRequest};

use crate::{
    AsyncTryIntoWithStore,
    extension::WasmState,
    wit::{
        AsyncTryFromWithStore, IntoHttpRequest,
        since_v0_1_0_draft::nero::extension::types::MediaResource,
    },
};

pub const MIN_VER: Version = Version::new(0, 1, 0);

bindgen!({
    path: "./wit/v0.1.0-draft",
    world: "nero:extension/extension",
    imports: { default: async | trappable, },
    exports: { default: async },
    with: {
        "wasi:http": wasmtime_wasi_http::bindings::http,
        "wasi:logging": nero_wasi_logging::logging,
        "nero:keyvalue-ttl": nero_keyvalue_ttl::keyvalue_ttl,
    },
});

pub fn linker(engine: &Engine) -> Result<Linker<WasmState>> {
    let mut linker = Linker::<WasmState>::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker).unwrap();
    wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker).unwrap();
    nero_wasi_logging::add_to_linker(&mut linker).unwrap();
    nero_keyvalue_ttl::add_to_linker(&mut linker, |s| KeyValueTTL::new(s.table())).unwrap();

    Ok(linker)
}

impl From<Filter> for crate::types::Filter {
    fn from(filter: Filter) -> Self {
        crate::types::Filter {
            id: filter.id,
            display_name: filter.display_name,
        }
    }
}

impl From<FilterCategory> for crate::types::FilterCategory {
    fn from(filter_category: FilterCategory) -> Self {
        crate::types::FilterCategory {
            id: filter_category.id,
            display_name: filter_category.display_name,
            filters: filter_category
                .filters
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

impl From<crate::types::SearchFilter> for SearchFilter {
    fn from(search_filter: crate::types::SearchFilter) -> Self {
        SearchFilter {
            id: search_filter.id,
            values: search_filter.values,
        }
    }
}

impl AsyncTryFromWithStore<SeriesPage> for crate::types::SeriesPage {
    async fn try_from_with_store(
        page: SeriesPage,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        let mut items = Vec::new();
        for series in page.series {
            items.push(series.try_into_with_store(store).await?);
        }
        Ok(crate::types::SeriesPage {
            items,
            has_next_page: page.has_next_page,
        })
    }
}

impl AsyncTryFromWithStore<Series> for crate::types::Series {
    async fn try_from_with_store(
        series: Series,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        Ok(crate::types::Series {
            id: series.id,
            title: series.title,
            poster_resource: match series.poster_resource {
                Some(mr) => Some(mr.try_into_with_store(store).await?),
                None => None,
            },
            synopsis: series.synopsis,
            r#type: series.type_,
        })
    }
}

impl AsyncTryFromWithStore<EpisodesPage> for crate::types::EpisodesPage {
    async fn try_from_with_store(
        page: EpisodesPage,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        let mut items = Vec::new();
        for episode in page.episodes {
            items.push(episode.try_into_with_store(store).await?);
        }
        Ok(crate::types::EpisodesPage {
            items,
            has_next_page: page.has_next_page,
        })
    }
}

impl AsyncTryFromWithStore<Episode> for crate::types::Episode {
    async fn try_from_with_store(
        episode: Episode,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        Ok(crate::types::Episode {
            id: episode.id,
            number: episode.number,
            title: episode.title,
            thumbnail_resource: match episode.thumbnail_resource {
                Some(mr) => Some(mr.try_into_with_store(store).await?),
                None => None,
            },
            description: episode.description,
        })
    }
}

impl AsyncTryFromWithStore<Video> for crate::types::Video {
    async fn try_from_with_store(
        video: Video,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        Ok(crate::types::Video {
            media_resource: video.media_resource.try_into_with_store(store).await?,
            server: video.server,
            resolution: video.resolution,
        })
    }
}

impl AsyncTryFromWithStore<Resource<HostOutgoingRequest>> for HostOutgoingRequest {
    async fn try_from_with_store(
        resource: Resource<HostOutgoingRequest>,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        let table = store.data_mut().table();
        let outgoing_request = table.delete(resource)?;
        Ok(outgoing_request)
    }
}

impl AsyncTryFromWithStore<MediaResource> for crate::types::MediaResource {
    async fn try_from_with_store(
        media_resource: MediaResource,
        store: &mut wasmtime::Store<WasmState>,
    ) -> Result<Self> {
        match media_resource {
            MediaResource::HttpRequest(resource) => {
                let outgoing: HostOutgoingRequest = resource.try_into_with_store(store).await?;
                let request = outgoing.into_http_request().await?;
                Ok(crate::types::MediaResource::HttpRequest(Box::new(request)))
            }
            MediaResource::MagnetUri(uri) => {
                MagnetURI::from_str(&uri)
                    .map_err(|e| anyhow::anyhow!("Failed to parse magnet URI: {:?}", e))?;
                Ok(crate::types::MediaResource::MagnetUri(uri))
            }
        }
    }
}
