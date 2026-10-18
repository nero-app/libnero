use bytes::Bytes;

pub type HttpRequest = http::Request<Option<Bytes>>;

#[derive(Debug, Clone)]
pub enum MediaResource {
    HttpRequest(Box<HttpRequest>),
    MagnetUri(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub cursor: Option<String>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            cursor: None,
        }
    }
}

pub type SeriesPage = Page<Series>;
pub type EpisodesPage = Page<Episode>;
pub type EntriesPage = Page<Entry>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchStatus {
    Watching,
    Completed,
    OnHold,
    Dropped,
    PlanToWatch,
}

#[derive(Debug, Clone, Default)]
pub struct EntryDetails {
    pub progress: u32,
    pub score: Option<u8>,
    pub repeat_count: u32,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub series: Series,
    pub status: WatchStatus,
    pub details: EntryDetails,
}

#[derive(Debug, Clone)]
pub struct Series {
    pub id: String,
    pub title: String,
    pub poster_resource: Option<MediaResource>,
    pub synopsis: Option<String>,
    pub r#type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Episode {
    pub id: String,
    pub number: u16,
    pub title: Option<String>,
    pub thumbnail_resource: Option<MediaResource>,
    pub description: Option<String>,
}

type Resolution = (u16, u16);

#[derive(Debug, Clone)]
pub struct Video {
    pub media_resource: MediaResource,
    pub server: String,
    pub resolution: Resolution,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Filter {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FilterCategory {
    pub id: String,
    pub display_name: String,
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchFilter {
    pub id: String,
    pub values: Vec<String>,
}
