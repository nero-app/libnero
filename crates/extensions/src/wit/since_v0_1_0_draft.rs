use std::{str::FromStr, sync::LazyLock};

use self::nero::extension::types::{
    Episode, EpisodesPage, Filter, FilterCategory, SearchFilter, Series, SeriesPage, Video,
};

use anyhow::Result;
use magnet_uri::MagnetURI;
use semver::Version;
use wasmtime::{
    Engine,
    component::{Linker, Resource, bindgen},
};
use wasmtime_wasi_http::{WasiHttpView, types::HostOutgoingRequest};

use crate::{
    AsyncTryIntoWithStore,
    extension::{ExtensionCtx, WasmState},
    wit::{
        AsyncTryFromWithStore, IntoHttpRequest,
        since_v0_1_0_draft::nero::extension::types::MediaResource,
    },
};

use tracker::exports::nero::extension::tracker::{EntriesPage, Entry, EntryDetails, WatchStatus};

pub(super) type Tracker = tracker::exports::nero::extension::tracker::Guest;
pub(super) type TrackerIndices = tracker::exports::nero::extension::tracker::GuestIndices;

pub static VERSION: LazyLock<Version> =
    LazyLock::new(|| Version::parse("0.1.0-draft-2026-10-15").expect("invalid version"));

bindgen!({
    path: "./wit/v0.1.0-draft",
    world: "nero:extension/extension",
    imports: { default: async | trappable, },
    exports: { default: async },
    with: {
        "wasi:http/types/outgoing-request": wasmtime_wasi_http::types::HostOutgoingRequest,
    },
});

mod tracker {
    use wasmtime::component::bindgen;

    bindgen!({
        path: "./wit/v0.1.0-draft",
        interfaces: "export nero:extension/tracker@0.1.0-draft-2026-10-15;",
        exports: { default: async },
        with: {
            "nero:extension/types@0.1.0-draft-2026-10-15": super::nero::extension::types,
        },
    });
}

pub fn linker(engine: &Engine, ctx: &ExtensionCtx) -> Result<Linker<WasmState>> {
    let mut linker = Linker::<WasmState>::new(engine);
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)?;
    wasmtime_wasi_http::add_only_http_to_linker_async(&mut linker)?;
    nero_wasi_logging::add_to_linker(&mut linker)?;
    if ctx.keyvalue.is_some() {
        nero_keyvalue_ttl::add_to_linker(&mut linker)?;
    }
    if ctx.portal.is_some() {
        nero_portal::add_to_linker(&mut linker)?;
    }

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
            cursor: page.cursor,
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
            cursor: page.cursor,
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

impl From<crate::types::WatchStatus> for WatchStatus {
    fn from(status: crate::types::WatchStatus) -> Self {
        match status {
            crate::types::WatchStatus::Watching => Self::Watching,
            crate::types::WatchStatus::Completed => Self::Completed,
            crate::types::WatchStatus::OnHold => Self::OnHold,
            crate::types::WatchStatus::Dropped => Self::Dropped,
            crate::types::WatchStatus::PlanToWatch => Self::PlanToWatch,
        }
    }
}

impl From<WatchStatus> for crate::types::WatchStatus {
    fn from(status: WatchStatus) -> Self {
        match status {
            WatchStatus::Watching => Self::Watching,
            WatchStatus::Completed => Self::Completed,
            WatchStatus::OnHold => Self::OnHold,
            WatchStatus::Dropped => Self::Dropped,
            WatchStatus::PlanToWatch => Self::PlanToWatch,
        }
    }
}

impl From<crate::types::EntryDetails> for EntryDetails {
    fn from(details: crate::types::EntryDetails) -> Self {
        Self {
            progress: details.progress,
            score: details.score,
            repeat_count: details.repeat_count,
            notes: details.notes,
        }
    }
}

impl From<EntryDetails> for crate::types::EntryDetails {
    fn from(details: EntryDetails) -> Self {
        Self {
            progress: details.progress,
            score: details.score,
            repeat_count: details.repeat_count,
            notes: details.notes,
        }
    }
}

impl AsyncTryFromWithStore<Entry> for crate::types::Entry {
    async fn try_from_with_store(
        entry: Entry,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            series: entry.series.try_into_with_store(store).await?,
            status: entry.status.into(),
            details: entry.details.into(),
        })
    }
}

impl AsyncTryFromWithStore<EntriesPage> for crate::types::EntriesPage {
    async fn try_from_with_store(
        page: EntriesPage,
        store: &mut wasmtime::Store<WasmState>,
    ) -> anyhow::Result<Self> {
        let mut items = Vec::with_capacity(page.entries.len());
        for entry in page.entries {
            items.push(entry.try_into_with_store(store).await?);
        }
        Ok(Self {
            items,
            cursor: page.cursor,
        })
    }
}
