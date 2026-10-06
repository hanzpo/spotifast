//! The account's last Home shelves on disk, so Home has rows at start-up
//! while Spotify is asked for fresh ones.

use serde::{Deserialize, Serialize};

use crate::api::models::{Artist, Episode, PlayHistory, Playlist, Show, Track};
use crate::model::{HomeData, Loadable};

const VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cache {
    version: u32,
    pub account_id: String,
    recently_played: Option<Vec<PlayHistory>>,
    top_artists: Option<Vec<Artist>>,
    top_tracks: Option<Vec<Track>>,
    recommendations: Option<Vec<Track>>,
    /// Made-for-you playlists by search term, in shelf order.
    discover: Vec<(String, Vec<Playlist>)>,
    podcasts: Vec<(Show, Vec<Episode>)>,
}

impl Cache {
    /// The shelves `home` has answers for; those still loading are left out.
    pub fn capture(account_id: String, home: &HomeData) -> Self {
        let mut discover: Vec<(String, Vec<Playlist>)> = home
            .discover
            .iter()
            .filter_map(|(term, shelf)| Some((term.clone(), shelf.get()?.clone())))
            .collect();
        discover.sort_by(|a, b| a.0.cmp(&b.0));
        Self {
            version: VERSION,
            account_id,
            recently_played: home.recently_played.get().cloned(),
            top_artists: home.top_artists.get().cloned(),
            top_tracks: home.top_tracks.get().cloned(),
            recommendations: home.recommendations.get().cloned(),
            discover,
            podcasts: home.podcasts.clone(),
        }
    }

    pub fn valid_for(&self, account: &str) -> bool {
        self.version == VERSION && self.account_id == account
    }

    /// Fills the shelves of `home` that have no answer yet. A shelf Spotify
    /// has already answered for keeps that answer.
    pub fn restore(self, home: &mut HomeData) {
        fill(&mut home.recently_played, self.recently_played);
        fill(&mut home.top_artists, self.top_artists);
        fill(&mut home.top_tracks, self.top_tracks);
        fill(&mut home.recommendations, self.recommendations);
        for (term, playlists) in self.discover {
            fill(home.discover.entry(term).or_default(), Some(playlists));
        }
        if home.podcasts.is_empty() {
            home.podcasts = self.podcasts;
        }
    }
}

fn fill<T>(shelf: &mut Loadable<T>, cached: Option<T>) {
    if let Some(cached) = cached
        && shelf.get().is_none()
    {
        *shelf = Loadable::Loaded(cached);
    }
}

pub async fn read(path: &std::path::Path, account: &str) -> Option<Cache> {
    let bytes = tokio::fs::read(path).await.ok()?;
    let cache: Cache = serde_json::from_slice(&bytes).ok()?;
    cache.valid_for(account).then_some(cache)
}

pub async fn write(path: &std::path::Path, cache: &Cache) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec(cache).map_err(std::io::Error::other)?;
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, bytes).await?;
    crate::util::replace_file(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: u32) -> Track {
        Track {
            uri: format!("spotify:track:{n}"),
            name: format!("Song {n}"),
            ..Default::default()
        }
    }

    fn home_with_shelves() -> HomeData {
        let mut home = HomeData {
            top_tracks: Loadable::Loaded(vec![track(1), track(2)]),
            top_artists: Loadable::Loaded(vec![Artist {
                name: "Someone".into(),
                ..Default::default()
            }]),
            recently_played: Loadable::Failed("offline".into()),
            ..Default::default()
        };
        home.discover.insert(
            "Daily Mix".into(),
            Loadable::Loaded(vec![Playlist {
                name: "Daily Mix 1".into(),
                ..Default::default()
            }]),
        );
        home.discover.insert("daylist".into(), Loadable::Loading);
        home
    }

    #[test]
    fn shelves_survive_a_round_trip_through_disk() {
        let cache = Cache::capture("alice".into(), &home_with_shelves());
        let decoded: Cache = serde_json::from_slice(&serde_json::to_vec(&cache).unwrap()).unwrap();
        assert_eq!(decoded, cache);

        let mut home = HomeData::default();
        decoded.restore(&mut home);
        assert_eq!(home.top_tracks.get().map(Vec::len), Some(2));
        assert_eq!(home.top_artists.get().map(Vec::len), Some(1));
        // A shelf that had failed is left to load, not stored as empty.
        assert!(home.recently_played.get().is_none());
        assert!(home.discover["Daily Mix"].get().is_some());
        assert!(!home.discover.contains_key("daylist"));
    }

    #[test]
    fn a_shelf_spotify_already_answered_keeps_that_answer() {
        let cache = Cache::capture("alice".into(), &home_with_shelves());
        let mut home = HomeData {
            top_tracks: Loadable::Loaded(vec![track(9)]),
            top_artists: Loadable::Loading,
            ..Default::default()
        };
        cache.restore(&mut home);
        assert_eq!(home.top_tracks.get().unwrap()[0].uri, "spotify:track:9");
        assert_eq!(home.top_artists.get().map(Vec::len), Some(1));
    }

    #[tokio::test]
    async fn another_accounts_or_a_damaged_cache_is_a_miss() {
        let dir = std::env::temp_dir().join(format!("spotifast-home-cache-{}", std::process::id()));
        let path = dir.join("home.json");
        let cache = Cache::capture("alice".into(), &home_with_shelves());
        write(&path, &cache).await.unwrap();
        assert_eq!(read(&path, "alice").await, Some(cache));
        assert_eq!(read(&path, "bob").await, None);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(read(&path, "alice").await, None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
