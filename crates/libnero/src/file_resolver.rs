use std::collections::HashMap;

use anitomy::OwnedElementObject;
use nero_extensions::{WasmExtension, types::Series};

// TODO: implement quality-based file selection when multiple files exist for the same series and episode
// consider adding configuration options to customize quality preferences?

// TODO: handle files in subdirectories (e.g., "Season 1/[SubsPlease] Series - E01.mkv")
// the current implementation may not correctly parse or match files organized in nested folder structures

const NOT_EPISODE_TYPES: [&str; 10] = [
    "op", "opening", "ncop", "ed", "ending", "nced", "pv", "preview", "trailer", "cm",
];

pub trait TorrentFileResolver {
    fn find_episode(
        &self,
        extension: &WasmExtension,
        series_id: &str,
        episode_number: u32,
    ) -> impl Future<Output = anyhow::Result<Option<String>>>;

    fn parse_all(&self) -> Vec<(String, OwnedElementObject)>;

    fn parse_episodes(&self) -> Vec<(String, OwnedElementObject)>;
}

impl TorrentFileResolver for Vec<String> {
    async fn find_episode(
        &self,
        extension: &WasmExtension,
        series_id: &str,
        episode_number: u32,
    ) -> anyhow::Result<Option<String>> {
        let parsed_episodes = self.parse_episodes();

        let mut grouped_episodes = HashMap::new();
        for (path, obj) in parsed_episodes {
            let key = title_key(&obj);
            grouped_episodes
                .entry(key)
                .or_insert_with(Vec::new)
                .push((path, obj));
        }

        for (_, files) in grouped_episodes {
            if let Some(target) =
                find_episode_in_group(extension, &files, series_id, episode_number).await?
            {
                return Ok(Some(target));
            }
        }

        Ok(None)
    }

    fn parse_all(&self) -> Vec<(String, OwnedElementObject)> {
        self.iter()
            .map(|path| {
                let elements = anitomy::parse(path);
                let obj = elements.iter().collect();
                (path.clone(), obj)
            })
            .collect()
    }

    fn parse_episodes(&self) -> Vec<(String, OwnedElementObject)> {
        self.parse_all()
            .into_iter()
            .filter(|(_, obj)| {
                obj.title.is_some()
                    && obj.episode.is_some()
                    && obj
                        .kind
                        .as_ref()
                        .is_none_or(|k| !NOT_EPISODE_TYPES.contains(&k.to_lowercase().as_str()))
            })
            .collect()
    }
}

fn title_key(obj: &OwnedElementObject) -> String {
    let mut key = obj.title.clone().unwrap_or_default();

    if let Some(year) = &obj.year {
        key.push_str(year);
    }

    if let Some(season) = &obj.season {
        key.push_str(&format!("S{season}"));
    }

    key
}

async fn find_episode_in_group(
    extension: &WasmExtension,
    files: &[(String, OwnedElementObject)],
    series_id: &str,
    episode_number: u32,
) -> anyhow::Result<Option<String>> {
    let representative = match files.first() {
        Some((_, obj)) => obj,
        None => return Ok(None),
    };

    if find_series_by_title(extension, representative, series_id)
        .await?
        .is_none()
    {
        return Ok(None);
    }

    let target = files
        .iter()
        .find(|(_, parsed)| {
            parsed
                .episode
                .as_ref()
                .and_then(|e| e.parse::<u32>().ok())
                .map(|e| e == episode_number)
                .unwrap_or(false)
        })
        .map(|(file, _)| file.clone());

    Ok(target)
}

async fn find_series_by_title(
    extension: &WasmExtension,
    parsed: &OwnedElementObject,
    series_id: &str,
) -> anyhow::Result<Option<Series>> {
    let alternative_titles = generate_alternative_titles(parsed);

    for title in alternative_titles {
        if let Some(series) = search_all_pages(extension, &title, series_id).await? {
            return Ok(Some(series));
        }
    }

    Ok(None)
}

async fn search_all_pages(
    extension: &WasmExtension,
    query: &str,
    series_id: &str,
) -> anyhow::Result<Option<Series>> {
    for page_number in 1.. {
        let result = extension.search(query, Some(page_number), vec![]).await?;
        if let Some(series) = result.items.into_iter().find(|s| s.id == series_id) {
            return Ok(Some(series));
        }

        if !result.has_next_page {
            break;
        }
    }

    Ok(None)
}

fn generate_alternative_titles(obj: &OwnedElementObject) -> Vec<String> {
    let mut titles = Vec::new();

    if let Some(title) = &obj.title {
        titles.push(title.clone());

        if let Some(season_str) = &obj.season
            && let Ok(season) = season_str.parse::<u32>()
        {
            let postfix = match season {
                1 => "st",
                2 => "nd",
                3 => "rd",
                _ => "th",
            };

            titles.push(format!("{title} {season}"));
            titles.push(format!("{title} {season}{postfix} Season"));
            titles.push(format!("{title} Season {season}"));
        }
    }

    titles
}
