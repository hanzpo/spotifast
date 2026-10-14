//! A whole track's sound, decoded ahead of playback for the word timing to
//! hear. It is read from the player's own copy in the audio cache, once
//! the player has fetched all of it, so it never competes with the music
//! for the network.

use std::io::{self, Read, Seek, SeekFrom};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail, ensure};
use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::{Session, SpotifyUri};
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem};
use librespot_playback::decoder::{AudioDecoder, SymphoniaDecoder};
use symphonia::core::io::MediaSource;
use symphonia::core::probe::Hint;

/// Where Spotify's Ogg files start, past a header of its own.
const OGG_HEADER_END: u64 = 0xa7;

/// The formats a song may be cached in.
const FORMATS: [AudioFileFormat; 7] = [
    AudioFileFormat::OGG_VORBIS_96,
    AudioFileFormat::OGG_VORBIS_160,
    AudioFileFormat::OGG_VORBIS_320,
    AudioFileFormat::MP3_96,
    AudioFileFormat::MP3_160,
    AudioFileFormat::MP3_256,
    AudioFileFormat::MP3_320,
];

/// How often to look for the player's copy of a song.
const POLL: Duration = Duration::from_secs(1);
/// How long to wait for it: a song made ready ahead waits for the one
/// before it to near its end.
const PATIENCE: Duration = Duration::from_secs(20 * 60);

/// An open track, ready to decode.
pub struct Track {
    file: AudioDecrypt<AudioFile>,
    format: AudioFileFormat,
    length: u64,
}

/// Opens the track at `uri` once the player has all of it in the cache, so
/// reading it takes nothing from the music. Words are only placed in songs
/// that are playing or next, and the player fetches those whole on its own.
pub async fn open(session: &Session, uri: &str) -> Result<Track> {
    let uri = SpotifyUri::from_uri(uri).map_err(|error| anyhow!("{uri}: {error}"))?;
    let SpotifyUri::Track { id } = uri.clone() else {
        bail!("only songs have words");
    };
    let item = AudioItem::get_file(session, uri.clone())
        .await
        .map_err(|error| anyhow!("{error}"))
        .context("reading the song's files")?;
    let available = FORMATS
        .iter()
        .filter_map(|format| item.files.get(format).map(|file| (*format, *file)))
        .collect::<Vec<_>>();
    ensure!(!available.is_empty(), "the song has no file to read");
    let Some(cache) = session.cache() else {
        return read(session, id, available[0]).await;
    };
    // The cache writes a file in place as a download finishes; one whose
    // size held across a look is whole.
    let cached = || {
        available.iter().find_map(|(format, file)| {
            let path = cache.file_path(*file)?;
            let length = std::fs::metadata(path).ok()?.len();
            Some(((*format, *file), length))
        })
    };
    let chosen = settled(cached, POLL, PATIENCE).await?;
    read(session, id, chosen).await
}

/// What `look` finds once it has found the same thing, and the same size of
/// it, twice in a row `poll` apart.
async fn settled<T: PartialEq>(
    mut look: impl FnMut() -> Option<(T, u64)>,
    poll: Duration,
    patience: Duration,
) -> Result<T> {
    let started = tokio::time::Instant::now();
    let mut seen = None;
    loop {
        let found = look();
        if found.is_some() && found == seen {
            return Ok(found.map(|(found, _)| found).expect("checked above"));
        }
        ensure!(started.elapsed() < patience, "the song was never played");
        seen = found;
        tokio::time::sleep(poll).await;
    }
}

/// Opens the file of `id` in `format`, to decode.
async fn read(
    session: &Session,
    id: librespot_core::SpotifyId,
    (format, file_id): (AudioFileFormat, librespot_core::FileId),
) -> Result<Track> {
    let rate = match format {
        AudioFileFormat::OGG_VORBIS_96 | AudioFileFormat::MP3_96 => 12 * 1024,
        AudioFileFormat::OGG_VORBIS_160 | AudioFileFormat::MP3_160 => 20 * 1024,
        AudioFileFormat::MP3_256 => 32 * 1024,
        _ => 40 * 1024,
    };
    let file = AudioFile::open(session, file_id, rate)
        .await
        .map_err(|error| anyhow!("{error}"))
        .context("opening the song")?;
    let length = file
        .get_stream_loader_controller()
        .map_err(|error| anyhow!("{error}"))?
        .len() as u64;
    let key = session
        .audio_key()
        .request(id, file_id)
        .await
        .map_err(|error| anyhow!("{error}"))
        .context("getting the song's key")?;
    Ok(Track {
        file: AudioDecrypt::new(Some(key), file),
        format,
        length,
    })
}

impl Track {
    /// Decodes the whole track to interleaved stereo at 44.1 kHz. Reading
    /// waits on the download, so this blocks; call it off the runtime.
    pub fn decode(self) -> Result<Vec<f32>> {
        let ogg = AudioFiles::is_ogg_vorbis(self.format);
        let offset = if ogg { OGG_HEADER_END } else { 0 };
        decode(
            Subfile::new(self.file, offset, self.length)?,
            AudioFiles::mime_type(self.format),
        )
    }
}

/// Decodes all of `source`, a file of the type `mime` names.
fn decode(source: impl MediaSource + 'static, mime: Option<&str>) -> Result<Vec<f32>> {
    let mut hint = Hint::new();
    if let Some(mime) = mime {
        hint.mime_type(mime);
    }
    let mut decoder = SymphoniaDecoder::new(source, hint).map_err(|error| anyhow!("{error}"))?;
    let mut samples = Vec::new();
    while let Some((_, packet)) = decoder
        .next_packet()
        .map_err(|error| anyhow!("{error}"))
        .context("decoding the song")?
    {
        if let Ok(decoded) = packet.samples() {
            samples.extend(decoded.iter().map(|&sample| sample as f32));
        }
    }
    Ok(samples)
}

/// The file from `offset`, as the decoder should see it.
struct Subfile<T> {
    stream: T,
    offset: u64,
    length: u64,
}

impl<T: Read + Seek> Subfile<T> {
    fn new(mut stream: T, offset: u64, length: u64) -> io::Result<Self> {
        stream.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            stream,
            offset,
            length,
        })
    }
}

impl<T: Read> Read for Subfile<T> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl<T: Seek> Seek for Subfile<T> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let position = match position {
            SeekFrom::Start(at) => SeekFrom::Start(at + self.offset),
            other => other,
        };
        Ok(self.stream.seek(position)?.saturating_sub(self.offset))
    }
}

impl<T: Read + Seek + Send + Sync> MediaSource for Subfile<T> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length - self.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_song_is_read_once_the_player_has_finished_writing_it() {
        // Not there, then growing as the cache writes it, then whole.
        let mut looks = [None, Some(("song", 10)), Some(("song", 20))].into_iter();
        let mut looked = 0;
        let found = settled(
            || {
                looked += 1;
                looks.next().unwrap_or(Some(("song", 20)))
            },
            POLL,
            PATIENCE,
        )
        .await
        .unwrap();
        assert_eq!(found, "song");
        assert_eq!(looked, 4, "read only after the size held");
    }

    #[tokio::test(start_paused = true)]
    async fn a_song_that_never_plays_is_given_up_on() {
        let found = settled(|| None::<((), u64)>, POLL, PATIENCE).await;
        assert!(found.is_err());
    }

    #[test]
    fn a_file_past_spotifys_header_decodes_whole() {
        // Spotify's Ogg files start with a header of their own; the file
        // the decoder sees starts after it.
        let tone = include_bytes!("testdata/tone.ogg");
        let mut file = vec![0xa5; OGG_HEADER_END as usize];
        file.extend_from_slice(tone);
        let length = file.len() as u64;
        let source = Subfile::new(io::Cursor::new(file), OGG_HEADER_END, length).unwrap();
        let samples = decode(source, Some("audio/ogg")).unwrap();
        // A second of stereo at 44.1 kHz, give or take the codec's edges.
        let frames = samples.len() / 2;
        assert!((43_000..=45_500).contains(&frames), "{frames} frames");
        let loudest = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(loudest > 0.05, "the tone is heard: {loudest}");
    }
}
