//! What the local API answers (see [`crate::server`]).
//!
//! Each method reads the app's state or queues the same [`Action`] the
//! window would, so a client gets the window's behaviour: optimistic
//! playback, the queue's rules, and the library's caches. Pages that are
//! still loading answer with `"loading": true`; the client asks again.
//!
//! Besides replies the server pushes events: `player` when playback
//! changes, `library` when the library or a saved state does, `auth` when
//! the account signs in or out, `queue` when the queue changes, and
//! `toast` with the app's messages.

use std::hash::{Hash, Hasher};

use serde_json::{Value, json};

use super::App;
use crate::api::models::{
    Album, Artist, ArtistRef, Episode, PlayableItem, Playlist, Show, Track, pick_image,
};
use crate::model::{Action, DiscographyFilter, Loadable, Page, RowContext, ToastKind};
use crate::player::RepeatMode;
use crate::server::Request;

/// Artwork sizes a client gets: a row's thumbnail, a card, a header.
const THUMB: u32 = 64;
const CARD: u32 = 300;
const COVER: u32 = 640;

/// What the server last pushed, so it pushes only news.
#[derive(Default)]
pub(super) struct Pushed {
    player: Option<(Value, std::time::Instant)>,
    library: u64,
    auth: u64,
    queue: u64,
    toast: Option<std::time::Instant>,
}

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
        "name": track.name,
        "artists": artists(&track.artists),
        "album": track.album.as_ref().map(|album| json!({
            "id": album.id,
            "name": album.name,
        })),
        "duration_ms": track.duration_ms,
        "explicit": track.explicit,
        "track_number": track.track_number,
        "disc_number": track.disc_number,
        "playable": track.is_playable,
        "image": track.album.as_ref().and_then(|album| pick_image(&album.images, THUMB)),
    })
}

fn episode(episode: &Episode, show: Option<&Show>) -> Value {
    let show = show.or(episode.show.as_ref());
    json!({
        "kind": "episode",
        "uri": episode.uri,
        "name": episode.name,
        "artists": [],
        "subtitle": show.map(|show| show.name.clone()),
        "show_id": show.map(|show| show.id.clone()),
        "duration_ms": episode.duration_ms,
        "explicit": episode.explicit,
        "image": pick_image(&episode.images, THUMB)
            .or_else(|| show.and_then(|show| pick_image(&show.images, THUMB))),
        "release_date": episode.release_date,
        "description": crate::util::strip_html(&episode.description),
        "resume_ms": episode.resume_ms(),
        "played": episode.resume_point.as_ref().map(|point| point.fully_played),
    })
}

fn item(value: &PlayableItem) -> Value {
    match value {
        PlayableItem::Track(each) => track(each),
        PlayableItem::Episode(each) => episode(each, None),
    }
}

fn card(
    kind: &str,
    id: &str,
    uri: &str,
    name: &str,
    subtitle: Option<String>,
    image: Option<&str>,
) -> Value {
    json!({
        "kind": kind,
        "id": id,
        "uri": uri,
        "name": name,
        "subtitle": subtitle,
        "image": image,
    })
}

fn playlist_card(playlist: &Playlist) -> Value {
    card(
        "playlist",
        &playlist.id,
        &playlist.uri,
        &playlist.name,
        playlist
            .description
            .as_deref()
            .map(crate::util::strip_html)
            .filter(|text| !text.is_empty())
            .or_else(|| Some(format!("By {}", playlist.owner_name()))),
        pick_image(&playlist.images, CARD),
    )
}

fn album_card(album: &Album) -> Value {
    let artists =
        crate::api::models::join_names(album.artists.iter().map(|artist| artist.name.as_str()));
    let year = album.year().map(|year| year.to_string());
    card(
        "album",
        &album.id,
        &album.uri,
        &album.name,
        Some(match year {
            Some(year) if !artists.is_empty() => format!("{artists} · {year}"),
            Some(year) => year,
            None => artists,
        }),
        pick_image(&album.images, CARD),
    )
}

fn artist_card(artist: &Artist) -> Value {
    card(
        "artist",
        &artist.id,
        &artist.uri,
        &artist.name,
        Some("Artist".into()),
        pick_image(&artist.images, CARD),
    )
}

fn show_card(show: &Show) -> Value {
    card(
        "show",
        &show.id,
        &show.uri,
        &show.name,
        Some(show.publisher.clone()),
        pick_image(&show.images, CARD),
    )
}

fn repeat_name(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "off",
        RepeatMode::Context => "context",
        RepeatMode::Track => "track",
    }
}

/// Whether a value is still on its way, why it failed, and what it is.
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

fn param_bool(params: &Value, name: &str) -> Result<bool, String> {
    params
        .get(name)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing \"{name}\""))
}

fn param_strings(params: &Value, name: &str) -> Vec<String> {
    params
        .get(name)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn fingerprint(parts: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    parts.hash(&mut hasher);
    hasher.finish()
}

/// A song as a client describes it: enough for the row it adds to show
/// its name, artists, album and cover before Spotify confirms it.
fn described_track(value: &Value) -> Option<PlayableItem> {
    let uri = value.get("uri")?.as_str()?.to_string();
    if !uri.starts_with("spotify:track:") {
        return None;
    }
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    let artists = value
        .get("artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .map(|artist| ArtistRef {
                    id: artist.get("id").and_then(Value::as_str).map(str::to_string),
                    name: artist
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    uri: None,
                })
                .collect()
        })
        .unwrap_or_default();
    let album = value
        .get("album")
        .filter(|album| album.is_object())
        .map(|album| Album {
            id: album
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: album
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            images: text("image")
                .map(|url| {
                    vec![crate::api::models::Image {
                        url,
                        ..Default::default()
                    }]
                })
                .unwrap_or_default(),
            ..Default::default()
        });
    Some(PlayableItem::Track(Track {
        id: uri.rsplit(':').next().map(str::to_string),
        name: text("name").unwrap_or_default(),
        duration_ms: value
            .get("duration_ms")
            .and_then(Value::as_u64)
            .and_then(|ms| u32::try_from(ms).ok())
            .unwrap_or_default(),
        explicit: value
            .get("explicit")
            .and_then(Value::as_bool)
            .unwrap_or_default(),
        artists,
        album,
        uri,
        ..Default::default()
    }))
}

/// Bare songs and episodes for actions that take items, built from URIs.
fn items_from_uris(uris: &[String]) -> Vec<PlayableItem> {
    uris.iter()
        .map(|uri| {
            if uri.starts_with("spotify:episode:") {
                PlayableItem::Episode(Episode {
                    uri: uri.clone(),
                    id: uri.rsplit(':').next().unwrap_or_default().to_string(),
                    ..Default::default()
                })
            } else {
                PlayableItem::Track(Track {
                    uri: uri.clone(),
                    id: uri.rsplit(':').next().map(str::to_string),
                    ..Default::default()
                })
            }
        })
        .collect()
}

impl App {
    /// Answers the API's requests and tells its clients what changed.
    /// Runs with the app's logic, before the queued actions apply.
    pub(super) fn serve_api(&mut self) {
        if crate::server::parent_gone() && !self.quit_requested {
            self.actions.push(Action::Quit);
        }
        let Some(server) = self.api.take() else {
            return;
        };
        for request in server.take_requests() {
            let result = self.answer(&request);
            server.reply(&request, result);
        }
        if server.has_clients() {
            self.push_events(&server);
        } else {
            self.api_pushed = Pushed::default();
        }
        self.api = Some(server);
    }

    fn push_events(&mut self, server: &crate::server::Server) {
        let player = self.api_player();
        // Position moves every pass; a client counts it on from the last
        // report, so only a jump or a change of anything else is news.
        let news = match &self.api_pushed.player {
            Some((sent, at)) => {
                let mut expected = sent.clone();
                if sent["playing"] == true
                    && let Some(position) = sent["position_ms"].as_u64()
                {
                    expected["position_ms"] = json!(position + at.elapsed().as_millis() as u64);
                }
                let drift = player["position_ms"]
                    .as_u64()
                    .zip(expected["position_ms"].as_u64())
                    .map_or(0, |(now, expected)| now.abs_diff(expected));
                let mut same = player.clone();
                same["position_ms"] = expected["position_ms"].clone();
                same != expected || drift > 2_000
            }
            None => true,
        };
        if news {
            server.broadcast("player", player.clone());
            self.api_pushed.player = Some((player, std::time::Instant::now()));
        }

        let auth = fingerprint((
            self.user.as_ref().map(|user| user.id.clone()),
            format!("{:?}", std::mem::discriminant(&self.auth)),
        ));
        if auth != self.api_pushed.auth {
            if self.api_pushed.auth != 0 {
                server.broadcast("auth", self.api_hello());
            }
            self.api_pushed.auth = auth;
        }

        let playlists = self
            .library
            .playlists
            .get()
            .map(|list| {
                list.iter()
                    .map(|playlist| {
                        (
                            playlist.id.clone(),
                            playlist.name.clone(),
                            playlist.snapshot_id.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut saved: Vec<_> = self
            .saved
            .iter()
            .map(|(uri, on)| (uri.clone(), *on))
            .collect();
        saved.sort();
        let library = fingerprint((
            playlists,
            saved,
            self.library.liked.total,
            self.library.albums.total,
            self.library.artists.items.len(),
            self.library.shows.total,
            self.rootlist.len(),
        ));
        if library != self.api_pushed.library {
            if self.api_pushed.library != 0 {
                server.broadcast("library", Value::Null);
            }
            self.api_pushed.library = library;
        }

        let queue = fingerprint(self.queue.get().map(|queue| {
            queue
                .queue
                .iter()
                .map(|item| item.uri().to_string())
                .collect::<Vec<_>>()
        }));
        if queue != self.api_pushed.queue {
            if self.api_pushed.queue != 0 {
                server.broadcast("queue", Value::Null);
            }
            self.api_pushed.queue = queue;
        }

        let since = self.api_pushed.toast;
        for toast in &self.toasts {
            if since.is_none_or(|since| toast.created > since) {
                server.broadcast(
                    "toast",
                    json!({
                        "message": toast.message,
                        "error": matches!(toast.kind, ToastKind::Error),
                    }),
                );
            }
        }
        if let Some(latest) = self.toasts.iter().map(|toast| toast.created).max() {
            self.api_pushed.toast = Some(latest.max(since.unwrap_or(latest)));
        } else if since.is_none() {
            self.api_pushed.toast = Some(std::time::Instant::now());
        }
    }

    fn answer(&mut self, request: &Request) -> Result<Value, String> {
        let params = &request.params;
        let ok = || Ok(json!({ "ok": true }));
        match request.method.as_str() {
            "hello" => Ok(self.api_hello()),
            "auth.sign_in" => {
                self.actions.push(Action::SignIn);
                ok()
            }
            "auth.cancel" => {
                self.actions.push(Action::CancelSignIn);
                ok()
            }
            "auth.sign_out" => {
                self.actions.push(Action::SignOut);
                ok()
            }
            "app.show_window" => {
                self.actions.push(Action::ShowWindow);
                ok()
            }
            "app.quit" => {
                self.actions.push(Action::Quit);
                ok()
            }
            "auth.enable_playback" => {
                self.actions.push(Action::EnablePlayback);
                ok()
            }

            // ---- the player ------------------------------------------------
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
                let done = params.get("done").and_then(Value::as_bool).unwrap_or(true);
                // Local volume is cheap to apply continuously; a remote
                // device's goes on release.
                if done {
                    self.volume_preview = None;
                    self.actions.push(Action::SetVolume(percent));
                } else {
                    self.volume_preview = Some(f32::from(percent) / 100.0);
                    if self.now_playing().is_none_or(|now| now.local) {
                        self.actions.push(Action::PreviewVolume(percent));
                    }
                }
                ok()
            }
            "player.mute" => {
                self.actions.push(Action::ToggleMute);
                ok()
            }
            "player.shuffle" => {
                self.actions
                    .push(Action::SetShuffle(param_bool(params, "on")?));
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
            // Plays a context (a playlist, an album, an artist, Liked
            // Songs' collection) from a song in it; a list of songs from
            // one of them; or a song or an episode on its own.
            "player.play" => {
                let context = params.get("context").and_then(Value::as_str);
                let uri = params.get("uri").and_then(Value::as_str);
                let uris = param_strings(params, "uris");
                let index = params
                    .get("index")
                    .and_then(Value::as_u64)
                    .and_then(|index| u32::try_from(index).ok());
                let action = match (context, uri) {
                    (Some(context), _) if context.starts_with("spotify:station:") => {
                        Action::PlayFromRow {
                            context: RowContext::View {
                                uris: uris.clone().into(),
                                context_uri: context.to_string(),
                                editable_playlist: None,
                            },
                            uri: uri.unwrap_or_default().to_string(),
                            index: index.unwrap_or(0),
                        }
                    }
                    // A sorted or filtered list plays in the order shown,
                    // still under its context's name.
                    (Some(context), Some(uri)) if !uris.is_empty() => Action::PlayFromRow {
                        context: RowContext::View {
                            uris: uris.clone().into(),
                            context_uri: context.to_string(),
                            editable_playlist: None,
                        },
                        uri: uri.to_string(),
                        index: index.unwrap_or(0),
                    },
                    (Some(context), _) => Action::PlayContext {
                        uri: context.to_string(),
                        offset_uri: uri.map(str::to_string),
                        offset_index: if uri.is_some() { None } else { index },
                    },
                    (None, _) if !uris.is_empty() => Action::PlayUris {
                        uris,
                        index: index.unwrap_or(0),
                    },
                    (None, Some(uri)) if uri.starts_with("spotify:episode:") => {
                        Action::PlayEpisode {
                            uri: uri.to_string(),
                            resume_ms: params
                                .get("resume_ms")
                                .and_then(Value::as_u64)
                                .and_then(|ms| u32::try_from(ms).ok()),
                        }
                    }
                    (None, Some(uri)) if uri.starts_with("spotify:track:") => Action::PlayUris {
                        uris: vec![uri.to_string()],
                        index: 0,
                    },
                    (None, Some(uri)) => Action::PlayContext {
                        uri: uri.to_string(),
                        offset_uri: None,
                        offset_index: None,
                    },
                    (None, None) => return Err("give \"context\", \"uris\" or \"uri\"".into()),
                };
                self.actions.push(action);
                ok()
            }

            // ---- the queue -------------------------------------------------
            "queue.get" => {
                if matches!(self.queue, Loadable::NotLoaded) {
                    self.actions.push(Action::RefreshQueue);
                }
                Ok(self.api_queue())
            }
            "queue.add" => {
                let songs: Vec<(String, String)> = params
                    .get("songs")
                    .and_then(Value::as_array)
                    .map(|songs| {
                        songs
                            .iter()
                            .filter_map(|song| {
                                Some((
                                    song.get("uri")?.as_str()?.to_string(),
                                    song.get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                ))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                if songs.is_empty() {
                    let uri = param_str(params, "uri")?;
                    let label = params
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    self.actions.push(Action::AddToQueue {
                        uri: uri.to_string(),
                        label: label.to_string(),
                    });
                } else {
                    self.actions.push(Action::QueueMany { songs });
                }
                ok()
            }
            // Plays a row of the queue: one of the songs queued by hand, or
            // one of the context's.
            "queue.play" => {
                let index = param_u64(params, "index")? as usize;
                let queued = params
                    .get("queued")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                let position = if queued {
                    index
                } else {
                    self.queued_rows_len() + index
                };
                let uri = self
                    .queue
                    .get()
                    .and_then(|queue| queue.queue.get(position))
                    .map(|item| item.uri().to_string())
                    .ok_or("no such row")?;
                self.actions.push(Action::PlayFromRow {
                    context: RowContext::Queue,
                    uri,
                    index: position as u32,
                });
                ok()
            }
            "queue.move" => {
                if !self.queue_locally_reorderable() {
                    return Err("The queue can be reordered only while playing on this Mac.".into());
                }
                let from = param_u64(params, "from")? as usize;
                let to = param_u64(params, "to")? as usize;
                self.actions.push(Action::MoveInQueue { from, to });
                ok()
            }
            "queue.clear" => {
                if !self.can_clear_queue() {
                    return Err("The queue can be cleared only while playing on this Mac.".into());
                }
                self.actions.push(Action::ClearQueue);
                ok()
            }
            "queue.save" => {
                self.actions.push(Action::SaveQueueAsPlaylist);
                ok()
            }

            // ---- lyrics and devices ---------------------------------------
            "lyrics.get" => {
                let playing = self.now_playing().map(|now| now.uri);
                if playing.is_some() && self.lyrics_uri != playing {
                    self.request_lyrics();
                }
                Ok(self.api_lyrics())
            }
            "devices.list" => Ok(self.api_devices()),
            "devices.refresh" => {
                self.actions.push(Action::RefreshDevices);
                ok()
            }
            "devices.transfer" => {
                self.actions
                    .push(Action::Transfer(param_str(params, "id")?.to_string()));
                ok()
            }

            // ---- the library -----------------------------------------------
            "library.playlists" => {
                // Asking again retries a load that failed, such as one
                // Spotify rate limited.
                if matches!(
                    self.library.playlists,
                    Loadable::NotLoaded | Loadable::Failed(_)
                ) && self.is_connected()
                {
                    self.load_playlists();
                }
                Ok(self.api_playlists())
            }
            "library.liked" => {
                self.touch_page(&Page::LikedSongs);
                self.ensure_loaded(Page::LikedSongs);
                Ok(self.api_liked())
            }
            "library.albums" => {
                self.touch_page(&Page::Albums);
                self.ensure_loaded(Page::Albums);
                let albums = &self.library.albums;
                Ok(json!({
                    "loading": albums.loading || (!albums.loaded_once && albums.items.is_empty()),
                    "error": albums.error,
                    "more": albums.can_load_more(),
                    "cards": albums.items.iter().map(|saved| album_card(&saved.album)).collect::<Vec<_>>(),
                }))
            }
            "library.artists" => {
                self.touch_page(&Page::Artists);
                self.ensure_loaded(Page::Artists);
                let artists = &self.library.artists;
                Ok(json!({
                    "loading": artists.loading || (!artists.loaded_once && artists.items.is_empty()),
                    "error": artists.error,
                    "more": artists.can_load_more(),
                    "cards": artists.items.iter().map(artist_card).collect::<Vec<_>>(),
                }))
            }
            "library.shows" => {
                self.touch_page(&Page::Podcasts);
                self.ensure_loaded(Page::Podcasts);
                let shows = &self.library.shows;
                Ok(json!({
                    "loading": shows.loading || (!shows.loaded_once && shows.items.is_empty()),
                    "error": shows.error,
                    "more": shows.can_load_more(),
                    "cards": shows
                        .items
                        .iter()
                        .filter(|saved| !self.audiobook_shows.contains(&saved.show.uri))
                        .map(|saved| show_card(&saved.show))
                        .collect::<Vec<_>>(),
                }))
            }
            // Saves a song, album, show, or artist (a follow), or follows a
            // playlist; or takes it back out.
            "library.episodes" => {
                self.touch_page(&Page::Episodes);
                self.ensure_loaded(Page::Episodes);
                let episodes = &self.library.episodes;
                Ok(json!({
                    "loading": episodes.loading || (!episodes.loaded_once && episodes.items.is_empty()),
                    "error": episodes.error,
                    "page": "episodes",
                    "kind": "Episodes",
                    "title": "Your Episodes",
                    "facts": [format!("{} episodes", episodes.total.unwrap_or(episodes.items.len() as u32))],
                    "more": episodes.can_load_more(),
                    "show_album": false,
                    "tracks": episodes.items.iter().map(|saved| {
                        let mut row = episode(&saved.episode, None);
                        row["added_at"] = json!(saved.added_at);
                        row
                    }).collect::<Vec<_>>(),
                }))
            }
            "search.history" => Ok(json!({ "queries": self.settings.search_history })),
            "search.forget" => {
                self.actions.push(Action::ForgetSearch(
                    param_str(params, "query")?.to_string(),
                ));
                ok()
            }
            "library.toggle" => {
                self.actions
                    .push(Action::ToggleSaved(param_str(params, "uri")?.to_string()));
                ok()
            }
            "library.set" => {
                let uris = param_strings(params, "uris");
                let saved = param_bool(params, "saved")?;
                self.actions.push(Action::SetSavedMany { uris, saved });
                ok()
            }
            "artist.follow" => {
                let id = param_str(params, "id")?;
                let uri = format!("spotify:artist:{id}");
                let follow = param_bool(params, "follow")?;
                if self.is_saved(&uri).unwrap_or(false) != follow {
                    self.actions.push(Action::ToggleSaved(uri));
                }
                ok()
            }

            // ---- pages -----------------------------------------------------
            "home.get" => {
                self.touch_page(&Page::Home);
                self.ensure_loaded(Page::Home);
                Ok(self.api_home())
            }
            "search" => {
                let query = param_str(params, "query")?.trim().to_string();
                if self.search.committed != query
                    || matches!(self.search.results, Loadable::NotLoaded)
                {
                    self.search.query = query.clone();
                    self.search.typed_at = None;
                    self.run_search(query.clone());
                }
                Ok(self.api_search(&query))
            }
            "playlist.get" => {
                let id = param_str(params, "id")?.to_string();
                self.open_quietly(Page::Playlist(id.clone()));
                Ok(self.api_playlist(&id))
            }
            "album.get" => {
                let id = param_str(params, "id")?.to_string();
                self.open_quietly(Page::Album(id.clone()));
                Ok(self.api_album(&id))
            }
            "artist.get" => {
                let id = param_str(params, "id")?.to_string();
                self.open_quietly(Page::Artist(id.clone()));
                Ok(self.api_artist(&id))
            }
            "show.get" => {
                let id = param_str(params, "id")?.to_string();
                self.open_quietly(Page::Show(id.clone()));
                Ok(self.api_show(&id))
            }
            "user.get" => {
                let id = param_str(params, "id")?.to_string();
                self.open_quietly(Page::User(id.clone()));
                Ok(self.api_user(&id))
            }
            "radio.get" => {
                let seed = param_str(params, "seed")?.to_string();
                self.open_quietly(Page::Radio(seed.clone()));
                Ok(self.api_radio(&seed))
            }
            "top_songs.get" => {
                self.open_quietly(Page::TopSongs);
                Ok(self.api_top_songs())
            }
            // Asks Spotify for the next page of a long list.
            "page.more" => {
                let page = Page::decode(param_str(params, "page")?).ok_or("unknown page")?;
                match page {
                    Page::Artist(id) => self.actions.push(Action::LoadMoreArtistAlbums(id)),
                    page => self.actions.push(Action::LoadMore(page)),
                }
                ok()
            }
            "page.reload" => {
                let page = Page::decode(param_str(params, "page")?).ok_or("unknown page")?;
                self.actions.push(Action::Reload(page));
                ok()
            }

            // ---- playlists -------------------------------------------------
            "playlist.create" => {
                self.actions.push(Action::CreatePlaylist {
                    name: param_str(params, "name")?.to_string(),
                    public: params
                        .get("public")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    add_uris: param_strings(params, "uris"),
                });
                ok()
            }
            "playlist.add" => {
                let id = param_str(params, "id")?.to_string();
                let name = self
                    .library
                    .playlists
                    .get()
                    .and_then(|list| list.iter().find(|playlist| playlist.id == id))
                    .map(|playlist| playlist.name.clone())
                    .unwrap_or_default();
                // Each song as the client describes it, or as this app has
                // already shown it, so the row it adds is whole at once.
                let mut items: Vec<PlayableItem> = params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(described_track).collect())
                    .unwrap_or_default();
                for uri in param_strings(params, "uris") {
                    let item = self
                        .known_song(&uri)
                        .unwrap_or_else(|| items_from_uris(std::slice::from_ref(&uri)).remove(0));
                    items.push(item);
                }
                // Songs already in the playlist go in again, as a client
                // that asks to add them means.
                self.actions.push(Action::ConfirmAddToPlaylist {
                    playlist_id: id,
                    playlist_name: name,
                    items,
                    position: None,
                });
                ok()
            }
            "playlist.remove" => {
                let id = param_str(params, "id")?.to_string();
                let mut uris: Vec<String> = params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.get("uri")?.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                uris.extend(param_strings(params, "uris"));
                uris.dedup();
                self.actions.push(Action::RemoveFromPlaylist {
                    playlist_id: id,
                    uris,
                });
                ok()
            }
            "playlist.move" => {
                self.actions.push(Action::MoveInPlaylist {
                    playlist_id: param_str(params, "id")?.to_string(),
                    from: param_u64(params, "from")? as u32,
                    to: param_u64(params, "to")? as u32,
                });
                ok()
            }
            "playlist.edit" => {
                let id = param_str(params, "id")?.to_string();
                let current = self
                    .library
                    .playlists
                    .get()
                    .and_then(|list| list.iter().find(|playlist| playlist.id == id))
                    .cloned();
                self.actions.push(Action::UpdatePlaylist {
                    name: params
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| current.as_ref().map(|playlist| playlist.name.clone()))
                        .unwrap_or_default(),
                    description: params
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            current
                                .as_ref()
                                .and_then(|playlist| playlist.description.clone())
                        })
                        .unwrap_or_default(),
                    public: params.get("public").and_then(Value::as_bool),
                    id,
                });
                ok()
            }
            "playlist.delete" => {
                self.actions
                    .push(Action::DeletePlaylist(param_str(params, "id")?.to_string()));
                ok()
            }
            "radio.save" => {
                self.actions
                    .push(Action::SaveRadio(param_str(params, "seed")?.to_string()));
                ok()
            }

            // ---- settings --------------------------------------------------
            "settings.get" => Ok(self.api_settings()),
            "settings.set" => {
                self.api_set_setting(
                    param_str(params, "key")?,
                    params.get("value").ok_or("missing \"value\"")?,
                )?;
                Ok(self.api_settings())
            }
            "settings.apply" => {
                self.actions.push(Action::RestartEngine);
                ok()
            }
            "settings.eq_preset" => {
                let index = param_u64(params, "index")? as usize;
                if index >= crate::eq::PRESETS.len() {
                    return Err("no such preset".into());
                }
                self.actions.push(Action::ApplyEqPreset(index));
                ok()
            }
            other => Err(format!("unknown method \"{other}\"")),
        }
    }

    /// Loads a page the way opening it would, without moving the window
    /// there.
    fn open_quietly(&mut self, page: Page) {
        self.touch_page(&page);
        self.ensure_loaded(page);
    }

    fn api_hello(&self) -> Value {
        json!({
            "app": "Spotlite",
            "version": env!("CARGO_PKG_VERSION"),
            "signed_in": self.user.is_some(),
            "auth": match &self.auth {
                crate::backend::AuthStatus::Starting => "starting",
                crate::backend::AuthStatus::SignedOut => "signed_out",
                crate::backend::AuthStatus::WaitingForBrowser { .. } => "waiting_for_browser",
                crate::backend::AuthStatus::Connecting => "connecting",
                crate::backend::AuthStatus::Connected { .. } => "connected",
                crate::backend::AuthStatus::Failed(_) => "failed",
            },
            "auth_error": match &self.auth {
                crate::backend::AuthStatus::Failed(error) => Some(error.clone()),
                _ => None,
            },
            "sign_in_url": self.sign_in_url,
            "user": self.user.as_ref().map(|user| json!({
                "id": user.id,
                "name": user.name(),
                "image": pick_image(&user.images, THUMB),
                "product": user.product,
            })),
        })
    }

    /// The player as a client shows it.
    fn api_player(&self) -> Value {
        let Some(now) = self.now_playing() else {
            return json!({
                "has_song": false,
                "shuffle": self.playing_context_shuffle(),
                "volume": crate::app::volume_to_percent(self.local.volume),
            });
        };
        let from = self.playing_from();
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
            "can_set_volume": now.can_set_volume,
            "local": now.local,
            "device": now.device_name,
            "is_episode": now.is_episode,
            "saved": self.is_saved(&now.uri),
            "context": self.playing_context_uri(),
            "playing_from": from.as_ref().map(|from| from.name.clone()),
            "playing_from_page": from.and_then(|from| from.page).map(|page| page.encode()),
        })
    }

    fn api_queue(&self) -> Value {
        let queued = self.queued_rows_len();
        let (loading, current, rows) = match &self.queue {
            Loadable::Loaded(queue) => (
                false,
                queue.currently_playing.as_ref().map(item),
                queue.queue.iter().map(item).collect::<Vec<_>>(),
            ),
            Loadable::Failed(_) => (false, None, Vec::new()),
            Loadable::Loading | Loadable::NotLoaded => (true, None, Vec::new()),
        };
        let split = queued.min(rows.len());
        json!({
            "loading": loading,
            "current": current,
            "queued": rows[..split],
            "next": rows[split..],
            "context": self.playing_from().map(|from| from.name),
            "reorderable": self.queue_locally_reorderable(),
            "clearable": self.can_clear_queue(),
        })
    }

    fn api_lyrics(&self) -> Value {
        let (loading, error, lyrics) = match &self.lyrics {
            Loadable::Loaded(lyrics) => (false, None, lyrics.as_ref()),
            Loadable::Failed(error) => (false, Some(error.clone()), None),
            Loadable::Loading | Loadable::NotLoaded => (self.now_playing().is_some(), None, None),
        };
        json!({
            "uri": self.lyrics_uri,
            "loading": loading,
            "error": error,
            "synced": lyrics.is_some_and(|lyrics| lyrics.synced),
            "instrumental": lyrics.is_some_and(|lyrics| lyrics.instrumental),
            "lines": lyrics
                .map(|lyrics| {
                    lyrics
                        .lines
                        .iter()
                        .map(|line| json!({ "time_ms": line.at_ms, "text": line.text }))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        })
    }

    fn api_devices(&self) -> Value {
        let mut devices: Vec<Value> = self
            .devices
            .iter()
            .filter_map(|device| {
                let id = device.id.clone()?;
                Some(json!({
                    "id": id,
                    "name": device.name,
                    "type": device.kind,
                    "active": device.is_active,
                    "volume": device.volume_percent,
                    "local": self.local_device_id.as_deref() == Some(id.as_str()),
                }))
            })
            .collect();
        // This Mac, which Spotify lists only once it has played here.
        if let Some(local) = &self.local_device_id
            && !self
                .devices
                .iter()
                .any(|device| device.id.as_deref() == Some(local))
        {
            devices.insert(
                0,
                json!({
                    "id": local,
                    "name": self.settings.device_name,
                    "type": "Computer",
                    "active": self.local.is_active(),
                    "volume": crate::app::volume_to_percent(self.local.volume),
                    "local": true,
                }),
            );
        }
        json!({ "loading": self.devices_loading, "devices": devices })
    }

    fn api_playlists(&self) -> Value {
        let user_id = self.user_id().unwrap_or_default().to_string();
        let shape = |playlist: &Playlist| {
            json!({
                "id": playlist.id,
                "uri": playlist.uri,
                "name": playlist.name,
                "description": playlist.description.as_deref().map(crate::util::strip_html),
                "owner": { "id": playlist.owner.id, "name": playlist.owner_name() },
                "image": pick_image(&playlist.images, THUMB),
                "cover": pick_image(&playlist.images, COVER),
                "total": playlist.track_total(),
                "editable": self.can_edit_playlist(playlist),
                "owned": playlist.owned_by(&user_id),
                "collaborative": playlist.collaborative,
                "public": playlist.public,
            })
        };
        let (loading, error, playlists) = loadable(&self.library.playlists, |list| {
            list.iter().map(shape).collect()
        });
        // The account's own order, with its folders.
        let rows = self.library.playlists.get().map(|list| {
            use crate::player::RootlistEntry;
            let find = |uri: &str| list.iter().find(|playlist| playlist.uri == uri);
            let mut stack: Vec<(Value, Vec<Value>)> = Vec::new();
            let mut top: Vec<Value> = Vec::new();
            let mut listed = std::collections::HashSet::new();
            for entry in &self.rootlist {
                match entry {
                    RootlistEntry::Playlist(uri) => {
                        if let Some(playlist) = find(uri) {
                            listed.insert(playlist.id.clone());
                            let row = json!({
                                "kind": "playlist",
                                "id": playlist.id,
                                "name": playlist.name,
                                "depth": stack.len(),
                                "playlist": shape(playlist),
                            });
                            match stack.last_mut() {
                                Some((_, children)) => children.push(row),
                                None => top.push(row),
                            }
                        }
                    }
                    RootlistEntry::FolderStart { id, name } => {
                        stack.push((
                            json!({ "kind": "folder", "id": id, "name": name, "depth": stack.len() }),
                            Vec::new(),
                        ));
                    }
                    RootlistEntry::FolderEnd => {
                        if let Some((mut folder, children)) = stack.pop() {
                            folder["children"] = json!(children);
                            match stack.last_mut() {
                                Some((_, siblings)) => siblings.push(folder),
                                None => top.push(folder),
                            }
                        }
                    }
                }
            }
            while let Some((mut folder, children)) = stack.pop() {
                folder["children"] = json!(children);
                top.push(folder);
            }
            // Playlists Spotify knows that the order does not list yet.
            for playlist in list.iter().filter(|playlist| !listed.contains(&playlist.id)) {
                top.push(json!({
                    "kind": "playlist",
                    "id": playlist.id,
                    "name": playlist.name,
                    "depth": 0,
                    "playlist": shape(playlist),
                }));
            }
            top
        });
        json!({
            "loading": loading,
            "error": error,
            "playlists": playlists,
            "rows": rows,
            "liked_context": self.liked_context(),
        })
    }

    fn liked_context(&self) -> Option<String> {
        self.user
            .as_ref()
            .map(|user| format!("spotify:user:{}:collection", user.id))
    }

    fn api_liked(&self) -> Value {
        let liked = &self.library.liked;
        let total = liked.total.unwrap_or(liked.items.len() as u32);
        json!({
            "loading": liked.loading || (!liked.loaded_once && liked.items.is_empty()),
            "error": liked.error,
            "page": "liked",
            "kind": "Playlist",
            "title": "Liked Songs",
            "owner": self.user.as_ref().map(|user| user.name().to_string()),
            "facts": [format!("{total} songs")],
            "context": self.liked_context(),
            "total": liked.total,
            "more": liked.can_load_more(),
            "show_album": true,
            "show_added": true,
            "tracks": liked.items.iter().map(|saved| {
                let mut row = track(&saved.track);
                row["added_at"] = json!(saved.added_at);
                row
            }).collect::<Vec<_>>(),
        })
    }

    fn api_playlist(&self, id: &str) -> Value {
        let Some(page) = self.playlist_pages.get(id) else {
            return json!({ "loading": true });
        };
        let user_id = self.user_id().unwrap_or_default().to_string();
        let header = page.playlist.get().or_else(|| {
            self.library
                .playlists
                .get()
                .and_then(|list| list.iter().find(|playlist| playlist.id == id))
        });
        let items = &page.items;
        let minutes: u64 = items
            .items
            .iter()
            .filter_map(|entry| entry.playable())
            .map(|item| u64::from(item.duration_ms()))
            .sum::<u64>()
            / 60_000;
        let total = items.total.or_else(|| header.map(Playlist::track_total));
        let mut facts = vec![if header.is_some_and(|playlist| playlist.collaborative) {
            "Collaborative playlist".to_string()
        } else if header.and_then(|playlist| playlist.public) == Some(true) {
            "Public playlist".to_string()
        } else {
            "Playlist".to_string()
        }];
        if let Some(total) = total {
            facts.push(format!("{total} songs"));
        }
        if items.is_complete() && minutes > 0 {
            facts.push(if minutes >= 60 {
                format!("{} hr {} min", minutes / 60, minutes % 60)
            } else {
                format!("{minutes} min")
            });
        }
        json!({
            "loading": page.playlist.is_loading() || items.loading || (!items.loaded_once && items.items.is_empty()),
            "error": match &page.playlist {
                Loadable::Failed(error) => Some(error.clone()),
                _ => items.error.clone(),
            },
            "page": Page::Playlist(id.to_string()).encode(),
            "kind": "Playlist",
            "title": header.map(|playlist| playlist.name.clone()),
            "owner": header.map(|playlist| playlist.owner_name().to_string()),
            "owner_route": header
                .and_then(|playlist| playlist.owner.id.clone())
                .map(|owner| format!("user:{owner}")),
            "description": header
                .and_then(|playlist| playlist.description.as_deref())
                .map(crate::util::strip_html),
            "cover": header.and_then(|playlist| pick_image(&playlist.images, COVER)),
            "facts": facts,
            "context": header.map(|playlist| playlist.uri.clone()),
            "uri": header.map(|playlist| playlist.uri.clone()),
            "saved": header.and_then(|playlist| self.is_saved(&playlist.uri)),
            "editable": header.is_some_and(|playlist| self.can_edit_playlist(playlist)),
            "owned": header.is_some_and(|playlist| playlist.owned_by(&user_id)),
            "total": total,
            "more": items.can_load_more(),
            "show_album": true,
            "show_added": true,
            "tracks": items.items.iter().enumerate().filter_map(|(index, entry)| {
                let mut row = item(entry.playable()?);
                row["added_at"] = json!(entry.added_at);
                // Where the song sits in Spotify's copy: rows it cannot
                // play are left out here, so the list's own order is not it.
                row["spotify_position"] = json!(items.base_offset as usize + index);
                if let Some(adder) = entry.added_by.as_ref().and_then(|user| user.id.as_ref())
                    && header.is_some_and(|playlist| playlist.collaborative
                        || playlist.owner.id.as_deref() != Some(adder.as_str()))
                {
                    row["added_by"] = json!(self
                        .user_names
                        .get(adder)
                        .cloned()
                        .flatten()
                        .unwrap_or_else(|| adder.clone()));
                }
                Some(row)
            }).collect::<Vec<_>>(),
        })
    }

    fn api_album(&self, id: &str) -> Value {
        let Some(page) = self.album_pages.get(id) else {
            return json!({ "loading": true });
        };
        let album = page.album.get();
        let tracks = &page.tracks;
        let minutes = tracks
            .items
            .iter()
            .map(|each| u64::from(each.duration_ms))
            .sum::<u64>()
            / 60_000;
        let mut facts = Vec::new();
        if let Some(album) = album {
            facts.push(self.album_kind_label(album).to_string());
            if let Some(year) = album.year() {
                facts.push(year.to_string());
            }
            if let Some(total) = album.total_tracks {
                facts.push(format!("{total} songs"));
            }
            if tracks.is_complete() && minutes > 0 {
                facts.push(format!("{minutes} min"));
            }
        }
        json!({
            "loading": page.album.is_loading() || tracks.loading || (!tracks.loaded_once && tracks.items.is_empty()),
            "error": match &page.album {
                Loadable::Failed(error) => Some(error.clone()),
                _ => tracks.error.clone(),
            },
            "page": Page::Album(id.to_string()).encode(),
            "kind": "Album",
            "title": album.map(|album| album.name.clone()),
            "owner": album.map(|album| crate::api::models::join_names(
                album.artists.iter().map(|artist| artist.name.as_str()))),
            "owner_route": album
                .and_then(|album| album.artists.first())
                .and_then(|artist| artist.id.clone())
                .map(|artist| format!("artist:{artist}")),
            "artists": album.map(|album| artists(&album.artists)),
            "description": album.and_then(|album| album.label.clone()).map(|label| format!("℗ {label}")),
            "cover": album.and_then(|album| pick_image(&album.images, COVER)),
            "facts": facts,
            "context": album.map(|album| album.uri.clone()),
            "uri": album.map(|album| album.uri.clone()),
            "saved": album.and_then(|album| self.is_saved(&album.uri)),
            "more": tracks.can_load_more(),
            "show_album": false,
            "tracks": tracks.items.iter().map(|each| {
                let mut row = track(each);
                // An album's songs come without the album; give each its cover.
                if let Some(album) = album {
                    row["album"] = json!({ "id": album.id, "name": album.name });
                    row["image"] = json!(pick_image(&album.images, THUMB));
                }
                row
            }).collect::<Vec<_>>(),
        })
    }

    fn api_artist(&self, id: &str) -> Value {
        let Some(page) = self.artist_pages.get(id) else {
            return json!({ "loading": true });
        };
        let group = |filter: DiscographyFilter| {
            page.albums
                .get(filter.groups())
                .map(|list| list.items.iter().map(album_card).collect::<Vec<_>>())
        };
        // The "All" list carries every kind; split it the way Spotify
        // groups an artist's releases.
        let all = page.albums.get(DiscographyFilter::All.groups());
        let of_type = |kind: &str| {
            all.map(|list| {
                list.items
                    .iter()
                    .filter(|album| {
                        album.album_group.as_deref().or(album.album_type.as_deref()) == Some(kind)
                    })
                    .map(album_card)
                    .collect::<Vec<_>>()
            })
        };
        let artist = page.artist.get();
        json!({
            "loading": page.artist.is_loading()
                || page.top_tracks.is_loading()
                || all.is_some_and(|list| list.loading),
            "error": match &page.artist {
                Loadable::Failed(error) => Some(error.clone()),
                _ => None,
            },
            "artist": artist.map(|artist| json!({
                "id": artist.id,
                "uri": artist.uri,
                "name": artist.name,
                "image": pick_image(&artist.images, THUMB),
                "cover": pick_image(&artist.images, COVER),
                "followers": artist.followers.as_ref().map(|followers| followers.total),
                "genres": artist.genres,
            })),
            "following": artist.and_then(|artist| self.is_saved(&artist.uri)),
            "top_tracks": page.top_tracks.get().map(|tracks| tracks.iter().map(track).collect::<Vec<_>>()),
            "albums": of_type("album"),
            "singles": of_type("single"),
            "compilations": of_type("compilation"),
            "appears_on": group(DiscographyFilter::AppearsOn).or_else(|| of_type("appears_on")),
            "related": page.related.get().map(|related| related.iter().map(artist_card).collect::<Vec<_>>()),
            "more": all.is_some_and(|list| list.can_load_more()),
        })
    }

    fn api_show(&self, id: &str) -> Value {
        let Some(page) = self.show_pages.get(id) else {
            return json!({ "loading": true });
        };
        let show = page.show.get();
        json!({
            "loading": page.show.is_loading() || page.episodes.loading || (!page.episodes.loaded_once && page.episodes.items.is_empty()),
            "error": match &page.show {
                Loadable::Failed(error) => Some(error.clone()),
                _ => page.episodes.error.clone(),
            },
            "show": show.map(|show| json!({
                "id": show.id,
                "uri": show.uri,
                "name": show.name,
                "publisher": show.publisher,
                "description": crate::util::strip_html(&show.description),
                "image": pick_image(&show.images, THUMB),
                "cover": pick_image(&show.images, COVER),
                "total": show.total_episodes,
            })),
            "saved": show.and_then(|show| self.is_saved(&show.uri)),
            "more": page.episodes.can_load_more(),
            "episodes": page.episodes.items.iter().map(|each| episode(each, show)).collect::<Vec<_>>(),
        })
    }

    fn api_user(&self, id: &str) -> Value {
        let Some(page) = self.user_pages.get(id) else {
            return json!({ "loading": true });
        };
        let (loading, error, user) = loadable(&page.profile, |profile| {
            json!({
                "user": {
                    "id": profile.id,
                    "name": profile.name,
                    "image": pick_image(&profile.images, COVER),
                    "followers": profile.followers,
                },
                "playlists": profile.playlists.iter().map(playlist_card).collect::<Vec<_>>(),
            })
        });
        json!({
            "loading": loading,
            "error": error,
            "user": user["user"],
            "playlists": user["playlists"],
        })
    }

    fn api_radio(&self, seed: &str) -> Value {
        let Some(page) = self.radio_pages.get(seed) else {
            return json!({ "loading": true });
        };
        let name = self.radio_name(seed);
        let images = self.radio_images(seed);
        let (loading, error, tracks) =
            loadable(&page.songs, |songs| songs.iter().map(track).collect());
        let uris: Vec<&str> = page
            .songs
            .get()
            .map(|songs| songs.iter().map(|song| song.uri.as_str()).collect())
            .unwrap_or_default();
        json!({
            "loading": loading,
            "error": error,
            "page": Page::Radio(seed.to_string()).encode(),
            "kind": "Radio",
            // The engine names a radio already: "Rosewood Radio".
            "title": name.unwrap_or_else(|| "Radio".into()),
            "facts": ["Radio", &format!("{} songs", uris.len())],
            "cover": pick_image(&images, COVER),
            "context": crate::util::station_uri(seed),
            "station_uris": uris,
            "seed": seed,
            "more": false,
            "show_album": true,
            "tracks": tracks,
        })
    }

    fn api_top_songs(&self) -> Value {
        let (loading, error, tracks) = loadable(&self.home.top_songs, |songs| {
            songs.iter().map(track).collect()
        });
        json!({
            "loading": loading || self.home.top_songs_loading,
            "error": error,
            "page": "top-songs",
            "kind": "Playlist",
            "title": "Your Top Songs",
            "facts": ["Last few months"],
            "more": false,
            "show_album": true,
            "tracks": tracks,
        })
    }

    fn api_home(&self) -> Value {
        let mut shelves = Vec::new();
        let home = &self.home;
        if self.settings.home.made_for_you.visible {
            let mut seen = std::collections::HashSet::new();
            let cards: Vec<Value> = crate::model::DISCOVER_TERMS
                .iter()
                .filter_map(|term| home.discover.get(*term)?.get())
                .flatten()
                .filter(|playlist| seen.insert(playlist.id.clone()))
                .map(playlist_card)
                .collect();
            if !cards.is_empty() {
                shelves
                    .push(json!({ "id": "made-for-you", "title": "Made for You", "cards": cards }));
            }
        }
        if let Some(recent) = home.recently_played.get() {
            let mut seen = std::collections::HashSet::new();
            let cards: Vec<Value> = recent
                .iter()
                .filter_map(|played| {
                    let album = played.track.album.as_ref()?;
                    seen.insert(album.id.clone()).then(|| album_card(album))
                })
                .take(20)
                .collect();
            if !cards.is_empty() {
                shelves.push(
                    json!({ "id": "recently-played", "title": "Recently Played", "cards": cards }),
                );
            }
        }
        let today = jiff::Zoned::now().date();
        let episodes: Vec<Value> = crate::ui::home::podcast_episodes(
            &home.podcasts,
            |show| self.audiobook_shows.contains(&show.uri),
            today,
        )
        .iter()
        .map(|(show, each, _)| episode(each, Some(show)))
        .collect();
        if !episodes.is_empty() {
            shelves.push(json!({ "id": "podcasts", "title": "Your Podcasts", "tracks": episodes }));
        }
        if let Some(artists) = home.top_artists.get().filter(|artists| !artists.is_empty()) {
            shelves.push(json!({
                "id": "top-artists",
                "title": "Your Top Artists",
                "cards": artists.iter().map(artist_card).collect::<Vec<_>>(),
            }));
        }
        if let Some(tracks) = home.top_tracks.get().filter(|tracks| !tracks.is_empty()) {
            shelves.push(json!({
                "id": "top-songs",
                "title": "Your Top Songs",
                "tracks": tracks.iter().take(10).map(track).collect::<Vec<_>>(),
                "see_all": "top-songs",
            }));
        }
        if self.settings.home.recommendations.visible
            && let Some(tracks) = home
                .recommendations
                .get()
                .filter(|tracks| !tracks.is_empty())
        {
            shelves.push(json!({
                "id": "recommended",
                "title": "Recommended for You",
                "tracks": tracks.iter().take(10).map(track).collect::<Vec<_>>(),
            }));
        }
        if let Some(playlists) = self.library.playlists.get().filter(|list| !list.is_empty()) {
            shelves.push(json!({
                "id": "your-playlists",
                "title": "Your Playlists",
                "cards": playlists.iter().take(20).map(playlist_card).collect::<Vec<_>>(),
            }));
        }
        let loading = home.recently_played.is_loading()
            || home.top_artists.is_loading()
            || home.top_tracks.is_loading();
        json!({ "loading": loading, "shelves": shelves })
    }

    fn api_search(&self, query: &str) -> Value {
        let search = &self.search;
        let results = search.results.get();
        let list = |each: Option<&crate::api::models::Page<Track>>| {
            each.map(|page| page.items.iter().map(track).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        let playlists = search
            .playlists
            .as_ref()
            .filter(|(serial, _)| *serial == search.serial)
            .map(|(_, page)| &page.items)
            .or_else(|| {
                results
                    .and_then(|results| results.playlists.as_ref())
                    .map(|page| &page.items)
            });
        json!({
            "query": if search.committed == query { query } else { search.committed.as_str() },
            "loading": search.results.is_loading() || search.catalogue_pending || search.playlists_pending,
            "error": search.error,
            "tracks": list(results.and_then(|results| results.tracks.as_ref())),
            "artists": results
                .and_then(|results| results.artists.as_ref())
                .map(|page| page.items.iter().map(artist_card).collect::<Vec<_>>())
                .unwrap_or_default(),
            "albums": results
                .and_then(|results| results.albums.as_ref())
                .map(|page| page.items.iter().map(album_card).collect::<Vec<_>>())
                .unwrap_or_default(),
            "playlists": playlists
                .map(|items| items.iter().map(playlist_card).collect::<Vec<_>>())
                .unwrap_or_default(),
            "shows": results
                .and_then(|results| results.shows.as_ref())
                .map(|page| {
                    page.items
                        .iter()
                        .filter(|show| !self.audiobook_shows.contains(&show.uri))
                        .map(show_card)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            "episodes": results
                .and_then(|results| results.episodes.as_ref())
                .map(|page| page.items.iter().map(|each| episode(each, None)).collect::<Vec<_>>())
                .unwrap_or_default(),
        })
    }

    fn api_settings(&self) -> Value {
        let settings = &self.settings;
        let toggle = |key: &str, label: &str, detail: &str, value: bool, group: &str| json!({ "key": key, "label": label, "detail": detail, "kind": "bool", "value": value, "group": group });
        let mut list = vec![
            json!({
                "key": "bitrate", "label": "Audio quality", "kind": "choice", "group": "Playback",
                "detail": "Applies when playback restarts.",
                "value": settings.bitrate.to_string(),
                "choices": [
                    { "value": "96", "label": "Normal (96 kbit/s)" },
                    { "value": "160", "label": "High (160 kbit/s)" },
                    { "value": "320", "label": "Very high (320 kbit/s)" },
                ],
            }),
            toggle(
                "normalisation",
                "Normalize volume",
                "Play every song at a similar loudness.",
                settings.normalisation,
                "Playback",
            ),
            toggle(
                "gapless",
                "Gapless playback",
                "Run songs into each other without a pause.",
                settings.gapless,
                "Playback",
            ),
            toggle(
                "autoplay",
                "Autoplay",
                "Keep playing similar songs when a list ends.",
                settings.autoplay,
                "Playback",
            ),
            toggle(
                "audio_cache",
                "Cache audio",
                "Keep recently played songs on disk.",
                settings.audio_cache,
                "Playback",
            ),
            json!({
                "key": "device_name", "label": "Device name", "kind": "text", "group": "Playback",
                "detail": "How this Mac appears in Spotify Connect.",
                "value": settings.device_name,
            }),
            toggle(
                "eq_on",
                "Equalizer",
                "Shape the sound of playback on this Mac.",
                settings.eq_on,
                "Equalizer",
            ),
            json!({
                "key": "eq_preamp_db", "label": "Preamp", "kind": "number", "group": "Equalizer",
                "value": settings.eq_preamp_db, "min": -crate::eq::RANGE_DB, "max": 0.0, "step": 0.5,
            }),
        ];
        for (index, hz) in crate::eq::BANDS.iter().enumerate() {
            let label = if *hz >= 1000.0 {
                format!("{} kHz", hz / 1000.0)
            } else {
                format!("{hz} Hz")
            };
            list.push(json!({
                "key": format!("eq_band_{index}"), "label": label, "kind": "number", "group": "Equalizer",
                "value": settings.eq_bands_db[index],
                "min": -crate::eq::RANGE_DB, "max": crate::eq::RANGE_DB, "step": 0.5,
            }));
        }
        list.push(toggle(
            "home_made_for_you",
            "Made for You on Home",
            "Discover Weekly, Release Radar and your mixes.",
            settings.home.made_for_you.visible,
            "Home",
        ));
        list.push(toggle(
            "home_recommendations",
            "Recommendations on Home",
            "Songs picked from what you listen to.",
            settings.home.recommendations.visible,
            "Home",
        ));
        json!({
            "settings": list,
            "eq_presets": crate::eq::PRESETS.iter().map(|preset| preset.name).collect::<Vec<_>>(),
        })
    }

    fn api_set_setting(&mut self, key: &str, value: &Value) -> Result<(), String> {
        let as_bool = || value.as_bool().ok_or("expected true or false");
        let as_number = || value.as_f64().ok_or("expected a number");
        match key {
            "bitrate" => {
                let bitrate = value
                    .as_str()
                    .and_then(|text| text.parse::<u16>().ok())
                    .or_else(|| value.as_u64().and_then(|n| u16::try_from(n).ok()))
                    .filter(|bitrate| [96, 160, 320].contains(bitrate))
                    .ok_or("expected 96, 160 or 320")?;
                self.settings.bitrate = bitrate;
            }
            "normalisation" => self.settings.normalisation = as_bool()?,
            "gapless" => self.settings.gapless = as_bool()?,
            "autoplay" => self.settings.autoplay = as_bool()?,
            "audio_cache" => self.settings.audio_cache = as_bool()?,
            "device_name" => {
                let name = value.as_str().ok_or("expected text")?.trim();
                if name.is_empty() {
                    return Err("the name can't be empty".into());
                }
                self.settings.device_name = name.to_string();
            }
            "eq_on" => {
                if as_bool()? != self.settings.eq_on {
                    self.actions.push(Action::ToggleEq);
                }
                return Ok(());
            }
            "eq_preamp_db" => {
                self.actions.push(Action::SetEqPreamp(as_number()? as f32));
                return Ok(());
            }
            "home_made_for_you" => self.settings.home.made_for_you.visible = as_bool()?,
            "home_recommendations" => self.settings.home.recommendations.visible = as_bool()?,
            band if band.starts_with("eq_band_") => {
                let index: usize = band["eq_band_".len()..]
                    .parse()
                    .ok()
                    .filter(|index| *index < crate::eq::BANDS.len())
                    .ok_or("no such band")?;
                self.actions
                    .push(Action::SetEqBand(index, as_number()? as f32));
                return Ok(());
            }
            other => return Err(format!("unknown setting \"{other}\"")),
        }
        self.actions.push(Action::SettingsChanged);
        Ok(())
    }
}
