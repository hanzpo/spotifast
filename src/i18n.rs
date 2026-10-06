//! Interface strings. Spotifast is English-only: these helpers return the
//! English source text, choosing the singular or plural form by count, and
//! keep the call sites free of hand-built plurals.

use std::borrow::Cow;

/// The interface language. English is the only one.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Locale {
    #[default]
    English,
}

/// The interface text for `source`.
pub fn gettext(_locale: Locale, source: &'static str) -> Cow<'static, str> {
    Cow::Borrowed(source)
}

/// The interface text for a phrase whose meaning depends on `context`.
pub fn pgettext(
    _locale: Locale,
    _context: &'static str,
    source: &'static str,
) -> Cow<'static, str> {
    Cow::Borrowed(source)
}

/// The singular form when `count` is one, otherwise the plural form.
pub fn ngettext(
    _locale: Locale,
    singular: &'static str,
    plural: &'static str,
    count: u32,
) -> Cow<'static, str> {
    Cow::Borrowed(if count == 1 { singular } else { plural })
}

impl Locale {
    pub fn liked_song_count(self, count: u32) -> String {
        ngettext(
            self,
            "Playlist • {count} song",
            "Playlist • {count} songs",
            count,
        )
        .replace("{count}", &count.to_string())
    }

    pub fn song_count(self, count: u32) -> String {
        ngettext(self, "{count} song", "{count} songs", count)
            .replace("{count}", &count.to_string())
    }

    pub fn playlist_count(self, count: u32) -> String {
        ngettext(self, "{count} playlist", "{count} playlists", count)
            .replace("{count}", &count.to_string())
    }

    pub fn folder_playlist_count(self, count: u32) -> String {
        ngettext(
            self,
            "Folder • {count} playlist",
            "Folder • {count} playlists",
            count,
        )
        .replace("{count}", &count.to_string())
    }

    pub fn folder_state_label(self, name: &str, collapsed: bool) -> String {
        let label = if collapsed {
            gettext(self, "{name}, folder, collapsed")
        } else {
            gettext(self, "{name}, folder, expanded")
        };
        label.replace("{name}", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_choose_the_singular_only_for_one() {
        for (count, expected) in [
            (0, "Playlist • 0 songs"),
            (1, "Playlist • 1 song"),
            (2, "Playlist • 2 songs"),
            (100_000, "Playlist • 100000 songs"),
        ] {
            assert_eq!(Locale::English.liked_song_count(count), expected);
        }
        assert_eq!(Locale::English.song_count(1), "1 song");
        assert_eq!(Locale::English.playlist_count(2), "2 playlists");
        assert_eq!(
            Locale::English.folder_playlist_count(1),
            "Folder • 1 playlist"
        );
        assert_eq!(
            Locale::English.folder_playlist_count(2),
            "Folder • 2 playlists"
        );
    }

    #[test]
    fn folder_state_labels_name_the_folder() {
        assert_eq!(
            Locale::English.folder_state_label("Road trips", true),
            "Road trips, folder, collapsed"
        );
        assert_eq!(
            Locale::English.folder_state_label("Road trips", false),
            "Road trips, folder, expanded"
        );
    }
}
