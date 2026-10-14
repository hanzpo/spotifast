//! A whole track's sound, decoded ahead of playback for the word timing to
//! hear. It is read as the player reads it, from the same files and the
//! same audio cache, so a song that is playing is usually read from disk.

use std::io::{self, Read, Seek, SeekFrom};

use anyhow::{Context, Result, anyhow, bail};
use librespot_audio::{AudioDecrypt, AudioFile};
use librespot_core::{Session, SpotifyUri};
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem};
use librespot_playback::decoder::{AudioDecoder, SymphoniaDecoder};
use symphonia::core::io::MediaSource;
use symphonia::core::probe::Hint;

/// Where Spotify's Ogg files start, past a header of its own.
const OGG_HEADER_END: u64 = 0xa7;

/// The formats to read, the least data first: the timing needs only the
/// voice, and a file the player already fetched is read from the cache
/// whatever its quality.
const FORMATS: [AudioFileFormat; 7] = [
    AudioFileFormat::OGG_VORBIS_96,
    AudioFileFormat::OGG_VORBIS_160,
    AudioFileFormat::OGG_VORBIS_320,
    AudioFileFormat::MP3_96,
    AudioFileFormat::MP3_160,
    AudioFileFormat::MP3_256,
    AudioFileFormat::MP3_320,
];

/// An open track, ready to decode.
pub struct Track {
    file: AudioDecrypt<AudioFile>,
    format: AudioFileFormat,
    length: u64,
}

/// Opens the track at `uri`, preferring a file the cache holds.
pub async fn open(session: &Session, uri: &str) -> Result<Track> {
    let uri = SpotifyUri::from_uri(uri).map_err(|error| anyhow!("{uri}: {error}"))?;
    let SpotifyUri::Track { id } = uri.clone() else {
        bail!("only songs have words");
    };
    let item = AudioItem::get_file(session, uri.clone())
        .await
        .map_err(|error| anyhow!("{error}"))
        .context("reading the song's files")?;
    let cache = session.cache();
    let available = FORMATS
        .iter()
        .filter_map(|format| item.files.get(format).map(|file| (*format, *file)))
        .collect::<Vec<_>>();
    let (format, file_id) = available
        .iter()
        .find(|(_, file)| {
            cache.is_some_and(|cache| cache.file_path(*file).is_some_and(|path| path.is_file()))
        })
        .or(available.first())
        .copied()
        .ok_or_else(|| anyhow!("the song has no file to read"))?;
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
