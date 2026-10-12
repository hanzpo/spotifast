//! What the local API answers (see [`crate::server`]).
//!
//! Each method reads the app's state or queues the same [`Action`] the
//! window would, so a client gets the window's behaviour: optimistic
//! playback, the queue's rules, and the library's caches. Lists that are
//! still loading answer with `"loading": true`; the client asks again.

use serde_json::{Value, json};

use super::App;
use crate::api::models::{Album, ArtistRef, PlayableItem, Playlist, Track, pick_image};
use crate::model::{Action, Loadable, Page};
use crate::player::RepeatMode;
use crate::server::Request;

/// Artwork sizes a client gets: a row's thumbnail and a header's cover.
const THUMB: u32 = 64;
const COVER: u32 = 640;

fn artists(artists: &[ArtistRef]) -> Value {
    artists
        .iter()
        .map(|artist| json!({ "id": artist.id, "name": artist.name }))
        .collect()
}

fn track(track: &Track) -> Value {
    json!({
        "kind": "track",
        "uri": track.uri,
        "id": track.id,
        "name": track.name,
        "artists": artists(&track.artists),
        "album": track.album.as_ref().map(|album| json!({
            "id": album.id,
            "name": album.name,
        })),
        "duration_ms": track.duration_ms,
        "explicit": track.explicit,
        "track_number": track.track_number,
        "image": track.album.as_ref().and_then(|album| pick_image(&album.images, THUMB)),
    })
}

fn item(item: &PlayableItem) -> Value {
    match item {
        PlayableItem::Track(each) => track(each),
        PlayableItem::Episode(_) => json!({
            "kind": "episode",
            "uri": item.uri(),
            "name": item.name(),
            "artists": [],
            "album": null,
            "subtitle": item.subtitle(),
            "duration_ms": item.duration_ms(),
            "explicit": false,
            "image": item.image(THUMB),
        }),
    }
}

fn playlist(playlist: &Playlist) -> Value {
    json!({
        "id": playlist.id,
        "uri": playlist.uri,
        "name": playlist.name,
        "description": playlist.description.as_deref().map(crate::util::strip_html),
        "owner": { "id": playlist.owner.id, "name": playlist.owner_name() },
        "image": pick_image(&playlist.images, THUMB),
        "cover": pick_image(&playlist.images, COVER),
        "total": playlist.track_total(),
    })
}

fn album(album: &Album) -> Value {
    json!({
        "id": album.id,
        "uri": album.uri,
        "name": album.name,
        "artists": artists(&album.artists),
        "year": album.year(),
        "image": pick_image(&album.images, THUMB),
        "cover": pick_image(&album.images, COVER),
        "total": album.total_tracks,
    })
}

fn repeat_name(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::Context => "context",
        RepeatMode::Track => "track",
    }
}

fn loadable<T>(
    value: &Loadable<T>,
    shape: impl FnOnce(&T) -> Value,
) -> (bool, Option<String>, Value) {
    match value {
        Loadable::Loaded(inner) => (false, None, shape(inner)),
        Loadable::Failed(error) => (false, Some(error.clone()), Value::Null),
        Loadable::Loading | Loadable::NotLoaded => (true, None, Value::Null),
    }
}

fn param_str<'a>(params: &'a Value, name: &str) -> Result<&'a str, String> {
    params
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| format!("missing \"{name}\""))
}

fn param_u64(params: &Value, name: &str) -> Result<u64, String> {
    params
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing \"{name}\""))
}

impl App {
    /// Answers the API's requests and tells its clients what changed.
    /// Runs with the app's logic, before the queued actions apply.
    pub(super) fn serve_api(&mut self) {
        let Some(server) = self.api.take() else {
            return;
        };
        for request in server.take_requests() {
            let result = self.answer(&request);
            server.reply(&request, result);
        }
        if server.has_clients() {
            let player = self.api_player();
            // Position moves every pass; a client counts it on from the
            // last report, so only a jump or a change of anything else is
            // news. Within two seconds of the expected position is no jump.
            let news = match &self.api_player_sent {
                Some((sent, at)) => {
                    let mut moved = sent.clone();
                    let elapsed = at.elapsed().as_millis() as u64;
                    if sent["playing"] == true
                        && let Some(position) = sent["position_ms"].as_u64()
                    {
                        moved["position_ms"] = json!(position + elapsed);
                    }
                    let drift = player["position_ms"]
                        .as_u64()
                        .zip(moved["position_ms"].as_u64())
                        .map_or(0, |(now, expected)| now.abs_diff(expected));
                    let mut same = player.clone();
                    same["position_ms"] = moved["position_ms"].clone();
                    same != moved || drift > 2_000
                }
                None => true,
            };
            if news {
                server.broadcast("player", player.clone());
                self.api_player_sent = Some((player, std::time::Instant::now()));
            }
        } else {
            self.api_player_sent = None;
        }
        self.api = Some(server);
    }

    fn answer(&mut self, request: &Request) -> Result<Value, String> {
        let params = &request.params;
        let ok = || Ok(json!({ "ok": true }));
        match request.method.as_str() {
            "hello" => Ok(json!({
                "app": "Spotlite",
                "version": env!("CARGO_PKG_VERSION"),
                "signed_in": self.user.is_some(),
                "user": self.user.as_ref().map(|user| json!({
                    "id": user.id,
                    "name": user.name(),
                    "image": pick_image(&user.images, THUMB),
                })),
            })),
            "player.state" => Ok(self.api_player()),
            "player.toggle" => {
                self.actions.push(Action::TogglePlay);
                ok()
            }
            "player.next" => {
                self.actions.push(Action::Next);
                ok()
            }
            "player.previous" => {
                self.actions.push(Action::Previous);
                ok()
            }
            "player.seek" => {
                let position = param_u64(params, "position_ms")?;
                self.actions
                    .push(Action::Seek(u32::try_from(position).unwrap_or(u32::MAX)));
                ok()
            }
            "player.volume" => {
                let percent = param_u64(params, "percent")?.min(100) as u8;
                self.actions.push(Action::SetVolume(percent));
                ok()
            }
            "player.shuffle" => {
                let on = params
                    .get("on")
                    .and_then(Value::as_bool)
                    .ok_or("missing \"on\"")?;
                self.actions.push(Action::SetShuffle(on));
                ok()
            }
            "player.repeat" => {
                let mode = match param_str(params, "mode")? {
                    "off" => RepeatMode::Off,
                    "context" => RepeatMode::Context,
                    "track" => RepeatMode::Track,
                    other => return Err(format!("unknown repeat mode \"{other}\"")),
                };
                self.actions.push(Action::SetRepeat(mode));
                ok()
            }
            "player.like" => {
                let uri = match params.get("uri").and_then(Value::as_str) {
                    Some(uri) => uri.to_string(),
                    None => self
                        .now_playing()
                        .map(|now| now.uri)
                        .ok_or("nothing is playing")?,
                };
                self.actions.push(Action::ToggleSaved(uri));
                ok()
            }
            // Plays a context (a playlist, an album, Liked Songs' collection)
            // from a song in it, or a song on its own.
            "player.play" => {
                let context = params.get("context").and_then(Value::as_str);
                let uri = params.get("uri").and_then(Value::as_str);
                let index = params
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| u32::try_from(index).ok());
                let action = match (context, uri) {
                    (Some(context), _) => Action::PlayContext {
                        uri: context.to_string(),
                        offset_uri: uri.map(str::to_string),
                        offset_index: index,
                    },
                    (None, Some(uri)) => Action::PlayContext {
                        uri: uri.to_string(),
                        offset_uri: None,
                        offset_index: None,
                    },
                    (None, None) => return Err("give \"context\" or \"uri\"".into()),
                };
                self.actions.push(action);
                ok()
            }
            "queue.add" => {
                let uri = param_str(params, "uri")?;
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.actions.push(Action::QueueMany {
                    songs: vec![(uri.to_string(), name.to_string())],
                });
                ok()
            }
            "library.playlists" => {
                if matches!(self.library.playlists, Loadable::NotLoaded) && self.is_connected() {
                    self.load_playlists();
                }
                let (loading, error, playlists) = loadable(&self.library.playlists, |list| {
                    list.iter().map(playlist).collect()
                });
                Ok(json!({
                    "loading": loading,
                    "error": error,
                    "playlists": playlists,
                    "liked_context": self.user.as_ref()
                        .map(|user| format!("spotify:user:{}:collection", user.id)),
                }))
            }
            "library.liked" => {
                self.touch_page(&Page::LikedSongs);
                self.ensure_loaded(Page::LikedSongs);
                let liked = &self.library.liked;
                Ok(json!({
                    "loading": liked.loading || !liked.loaded_once,
                    "error": liked.error,
                    "total": liked.total,
                    "more": liked.can_load_more(),
                    "context": self.user.as_ref()
                        .map(|user| format!("spotify:user:{}:collection", user.id)),
                    "tracks": liked.items.iter().map(|saved| {
                        let mut row = track(&saved.track);
                        row["added_at"] = json!(saved.added_at);
                        row
                    }).collect::<Vec<_>>(),
                }))
            }
            "library.albums" => {
                self.touch_page(&Page::Albums);
                self.ensure_loaded(Page::Albums);
                let albums = &self.library.albums;
                Ok(json!({
                    "loading": albums.loading || !albums.loaded_once,
                    "error": albums.error,
                    "more": albums.can_load_more(),
                    "albums": albums.items.iter().map(|saved| album(&saved.album)).collect::<Vec<_>>(),
                }))
            }
            "playlist.get" => {
                let id = param_str(params, "id")?.to_string();
                let page = Page::Playlist(id.clone());
                self.touch_page(&page);
                self.ensure_loaded(page);
                let Some(page) = self.playlist_pages.get(&id) else {
                    return Ok(json!({ "loading": true }));
                };
                let (loading, error, header) = loadable(&page.playlist, playlist);
                let items = &page.items;
                Ok(json!({
                    "loading": loading || items.loading || !items.loaded_once,
                    "error": error.or_else(|| items.error.clone()),
                    "playlist": header,
                    "total": items.total,
                    "more": items.can_load_more(),
                    "tracks": items.items.iter().filter_map(|entry| {
                        let mut row = item(entry.playable()?);
                        row["added_at"] = json!(entry.added_at);
                        Some(row)
                    }).collect::<Vec<_>>(),
                }))
            }
            "album.get" => {
                let id = param_str(params, "id")?.to_string();
                let page = Page::Album(id.clone());
                self.touch_page(&page);
                self.ensure_loaded(page);
                let Some(page) = self.album_pages.get(&id) else {
                    return Ok(json!({ "loading": true }));
                };
                let (loading, error, header) = loadable(&page.album, album);
                let tracks = &page.tracks;
                Ok(json!({
                    "loading": loading || tracks.loading || !tracks.loaded_once,
                    "error": error.or_else(|| tracks.error.clone()),
                    "album": header,
                    "more": tracks.can_load_more(),
                    "tracks": tracks.items.iter().map(track).collect::<Vec<_>>(),
                }))
            }
            // Asks Spotify for the next page of a long list.
            "page.more" => {
                let page = Page::decode(param_str(params, "page")?).ok_or("unknown page")?;
                self.actions.push(Action::LoadMore(page));
                ok()
            }
            other => Err(format!("unknown method \"{other}\"")),
        }
    }

    /// The player as a client shows it.
    fn api_player(&self) -> Value {
        let Some(now) = self.now_playing() else {
            return json!({ "has_song": false });
        };
        json!({
            "has_song": true,
            "uri": now.uri,
            "title": now.title,
            "subtitle": now.subtitle,
            "artists": artists(&now.artists),
            "album": { "id": now.album_id, "name": now.album_name },
            "show_id": now.show_id,
            "image": now.art_small.as_deref().or(now.art_url.as_deref()),
            "cover": now.art_url.as_deref().or(now.art_small.as_deref()),
            "duration_ms": now.duration_ms,
            "position_ms": now.position_ms,
            "playing": now.playing,
            "loading": now.loading,
            "shuffle": now.shuffle,
            "repeat": repeat_name(now.repeat),
            "volume": now.volume_percent,
            "local": now.local,
            "device": now.device_name,
            "is_episode": now.is_episode,
            "saved": self.is_saved(&now.uri),
            "context": self.playing_context_uri(),
        })
    }
}
