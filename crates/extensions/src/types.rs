use bytes::Bytes;

pub type HttpRequest = http::Request<Option<Bytes>>;

#[derive(Debug, Clone)]
pub enum MediaResource {
    HttpRequest(Box<HttpRequest>),
    MagnetUri(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub has_next_page: bool,
}

pub type SeriesPage = Page<Series>;
pub type EpisodesPage = Page<Episode>;

#[derive(Debug, Clone, Default)]
pub struct Series {
    pub id: String,
    pub title: String,
    pub poster_resource: Option<MediaResource>,
    pub synopsis: Option<String>,
    pub r#type: Option<String>,
}

#[derive(Debug, Clone, Default)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Filter {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FilterCategory {
    pub id: String,
    pub display_name: String,
    pub filters: Vec<Filter>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SearchFilter {
    pub id: String,
    pub values: Vec<String>,
}
