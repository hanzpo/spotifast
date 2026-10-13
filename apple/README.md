# SwiftSpot for SwiftUI

A native SwiftUI front end. It doesn't talk to Spotify itself: the running
SwiftSpot is the server, keeping the session, playback, the queue and the
library, and this app drives it over SwiftSpot's local API.

## Run it

1. Start SwiftSpot as usual (the installed app, or `cargo run`).
2. `cd apple && swift run`

It connects to `~/Library/Application Support/me.paolino.spotifast/api.sock`.
To try it without an account, run the demo and point the app at the demo's
socket:

```sh
cargo run --features demo -- --demo &
SWIFTSPOT_SOCKET=$(ls -d "$TMPDIR"spotifast-demo-*/state/api.sock | tail -1) swift run
```

## The API

One JSON object per line over the Unix socket (`src/server.rs`). A client
sends `{"id": 1, "method": "…", "params": {…}}` and gets back
`{"id": 1, "result": …}` or `{"id": 1, "error": "…"}`. The server also
pushes `{"event": "player", "data": …}` whenever playback changes, apart
from the position simply moving on.

| Method | Params | Answers |
| --- | --- | --- |
| `hello` | | app, version, signed-in user |
| `player.state` | | the player (same shape as the `player` event) |
| `player.toggle`, `player.next`, `player.previous` | | |
| `player.seek` | `position_ms` | |
| `player.volume` | `percent` | |
| `player.shuffle` | `on` | |
| `player.repeat` | `mode`: `off`, `context`, `track` | |
| `player.like` | `uri`? (the playing song by default) | |
| `player.play` | `context`? `uri`? `index`? | |
| `karaoke.set` | `on`? `level`? (voice kept, 0 to 1) | |
| `queue.add` | `uri`, `name`? | |
| `library.playlists` | | `playlists`, `liked_context` |
| `library.liked` | | `tracks`, `total`, `more`, `context` |
| `library.albums` | | `albums`, `more` |
| `playlist.get` | `id` | `playlist`, `tracks`, `total`, `more` |
| `album.get` | `id` | `album`, `tracks`, `more` |
| `page.more` | `page`, e.g. `playlist:ID`, `liked` | |

The player carries `karaoke`: `available`, `on`, `level`, and `status`
(`off`, `downloading`, `preparing`, `running` or `failed`, with `error`).

Lists SwiftSpot is still fetching answer with `"loading": true`; ask again.
The handlers are in `src/app/api.rs`: each reads SwiftSpot's state or queues
the same action its window would, so a client gets the window's behaviour.
