//! Word-by-word lyrics: when each word, and each letter of it, is sung.
//!
//! Spotify's lyrics give each line a start time. To light the words up as
//! they are sung, the song is decoded ahead of playback, the voice taken
//! out of it with karaoke's model when it is there, and a speech model
//! hears it as letters; the lyrics are then lined up against those letters
//! (see [`align`]). It all happens on the Mac, once a song, and the result
//! is kept beside the lyrics.

pub mod align;
pub mod audio;
#[cfg(target_os = "macos")]
pub mod coreml;

use std::path::{Path, PathBuf};

use anyhow::Result;
use sha1::{Digest, Sha1};

pub use align::Word;
use align::{Emissions, FRAME_MS};

use crate::karaoke::Separator;

/// Each line's words with their marks, or `None` for a line not placed.
pub type Lines = Vec<Option<Vec<Word>>>;

/// The sample rate songs are decoded at.
const SONG_RATE: u32 = 44_100;
/// How much of each separation window's edges is let go: the model hears
/// less of what surrounds a sound there.
const SEPARATION_EDGE: usize = SONG_RATE as usize / 2;
/// The voice sounds when it is this loud against the song's loud singing
/// (its ninetieth percentile): eighteen decibels down.
const VOICED_BELOW_LOUD: f32 = 0.126;

/// Where a song's words are kept, for these lyrics: other lyrics for the
/// same song are placed afresh.
pub fn cache_path(dir: &Path, uri: &str, lines: &[(u32, &str)]) -> PathBuf {
    let mut hash = Sha1::new();
    for (at, text) in lines {
        hash.update(at.to_le_bytes());
        hash.update(text.as_bytes());
        hash.update([0]);
    }
    let id = uri.rsplit(':').next().unwrap_or(uri);
    let digest = hash.finalize();
    let short: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    dir.join(format!("words-{id}-{short}.json"))
}

pub fn read_cache(path: &Path) -> Option<Lines> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub fn write_cache(path: &Path, lines: &Lines) {
    let Ok(bytes) = serde_json::to_vec(lines) else {
        return;
    };
    let partial = path.with_extension("partial");
    if std::fs::write(&partial, bytes).is_ok() {
        std::fs::rename(&partial, path).ok();
    }
}

/// Places the lyrics' words in a song, decoded as interleaved stereo at
/// 44.1 kHz. With a separator, the model hears only the voice, and a held
/// note can be heard to end; without one it hears the whole song.
/// `emissions` turns mono 16 kHz sound into the letter model's emissions.
pub fn place<S: Separator + ?Sized>(
    song: &[f32],
    separator: Option<&mut S>,
    emissions: impl FnOnce(&[f32]) -> Result<Emissions>,
    lines: &[(u32, &str)],
) -> Result<Lines> {
    let (voice, voiced) = match separator {
        Some(separator) => {
            let vocals = separate(song, separator)?;
            let mono = mono(&vocals);
            let voiced = voiced(&mono);
            (mono, Some(voiced))
        }
        None => (mono(song), None),
    };
    let heard = emissions(&resample(&voice))?;
    Ok(align::align(&heard, lines, voiced.as_deref()))
}

/// The voice of a whole song, separated a window at a time. The windows
/// overlap, and each keeps only its middle.
fn separate<S: Separator + ?Sized>(song: &[f32], separator: &mut S) -> Result<Vec<f32>> {
    let window = crate::karaoke::WINDOW;
    let frames = song.len() / 2;
    let mut voice = vec![0.0; frames * 2];
    let step = window - 2 * SEPARATION_EDGE;
    let mut start = 0usize;
    let mut buffer = vec![0.0; window * 2];
    loop {
        // The window reaches an edge's width back, so its middle starts here.
        let from = start.saturating_sub(SEPARATION_EDGE);
        let available = frames.saturating_sub(from).min(window);
        buffer.fill(0.0);
        buffer[..available * 2].copy_from_slice(&song[from * 2..(from + available) * 2]);
        let vocals = separator.vocals(&buffer)?;
        let keep_from = start - from;
        let keep_to = (keep_from + step).min(available);
        voice[start * 2..(from + keep_to) * 2].copy_from_slice(&vocals[keep_from * 2..keep_to * 2]);
        start = from + keep_to;
        if start >= frames {
            break;
        }
    }
    Ok(voice)
}

fn mono(stereo: &[f32]) -> Vec<f32> {
    stereo
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[left, right]| (left + right) / 2.0)
        .collect()
}

/// Whether the voice sounds in each emission frame.
fn voiced(voice: &[f32]) -> Vec<bool> {
    let size = (SONG_RATE * FRAME_MS / 1000) as usize;
    let loudness: Vec<f32> = voice
        .chunks(size)
        .map(|frame| (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt())
        .collect();
    let mut sorted = loudness.clone();
    sorted.sort_by(f32::total_cmp);
    let loud = sorted
        .get(sorted.len() * 9 / 10)
        .copied()
        .unwrap_or_default();
    let floor = (loud * VOICED_BELOW_LOUD).max(1e-4);
    loudness.into_iter().map(|level| level > floor).collect()
}

/// Mono 44.1 kHz down to the letter model's 16 kHz.
fn resample(mono: &[f32]) -> Vec<f32> {
    let target = 16_000;
    let Some(mut resampler) = crate::resample::Resampler::new(SONG_RATE, target, 1) else {
        return mono.to_vec();
    };
    let mut out = resampler.process(mono);
    // The filter holds back its last few samples until more come.
    out.extend(resampler.process(&[0.0; 64]));
    out.truncate(mono.len() * target as usize / SONG_RATE as usize);
    out
}

/// A song to place the words of, and where to say how it went.
#[cfg(target_os = "macos")]
pub struct Job {
    pub track: Option<audio::Track>,
    pub lines: Vec<(u32, String)>,
    /// Made ready ahead of the song: the playing song's job goes first.
    pub ahead: bool,
    /// The letter model's package.
    pub aligner: PathBuf,
    /// Karaoke's vocal model's package, when it is on disk.
    pub separator: Option<PathBuf>,
    pub cache: PathBuf,
    pub done: Box<dyn FnOnce(Result<Lines>) + Send>,
}

/// Places words on a thread of its own, a song at a time. The models stay
/// loaded while songs keep coming and are let go once they stop.
#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct Placer {
    jobs: std::sync::mpsc::Sender<Job>,
}

#[cfg(target_os = "macos")]
impl Placer {
    /// How long the models are kept with nothing to do.
    const IDLE: std::time::Duration = std::time::Duration::from_secs(120);

    pub fn spawn() -> Self {
        let (jobs, queue) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("words".into())
            .spawn(move || {
                let mut aligner: Option<coreml::Aligner> = None;
                let mut separator: Option<Box<dyn Separator>> = None;
                let mut pending: Vec<Job> = Vec::new();
                loop {
                    if pending.is_empty() {
                        match queue.recv_timeout(Self::IDLE) {
                            Ok(job) => pending.push(job),
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                                aligner = None;
                                separator = None;
                                continue;
                            }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    }
                    pending.extend(queue.try_iter());
                    let mut job = next_job(&mut pending);
                    let result = run(&mut job, &mut aligner, &mut separator);
                    if let Ok(lines) = &result {
                        write_cache(&job.cache, lines);
                    }
                    (job.done)(result);
                }
            })
            .expect("the words thread starts");
        Self { jobs }
    }

    pub fn place(&self, job: Job) {
        let _ = self.jobs.send(job);
    }
}

/// The job to run next: the latest song asked for while it plays, or else
/// the latest asked for ahead. Older ones of the same kind are let go.
#[cfg(target_os = "macos")]
fn next_job(pending: &mut Vec<Job>) -> Job {
    let ahead = pending.iter().all(|job| job.ahead);
    let chosen = pending
        .iter()
        .rposition(|job| job.ahead == ahead)
        .expect("a job is pending");
    let job = pending.remove(chosen);
    let (stale, kept): (Vec<Job>, Vec<Job>) =
        pending.drain(..).partition(|other| other.ahead == ahead);
    *pending = kept;
    for stale in stale {
        (stale.done)(Err(anyhow::anyhow!("another song came first")));
    }
    job
}

#[cfg(target_os = "macos")]
fn run(
    job: &mut Job,
    aligner: &mut Option<coreml::Aligner>,
    separator: &mut Option<Box<dyn Separator>>,
) -> Result<Lines> {
    let started = std::time::Instant::now();
    let song = job
        .track
        .take()
        .ok_or_else(|| anyhow::anyhow!("the song was already read"))?
        .decode()?;
    let decoded = started.elapsed();
    if separator.is_none()
        && let Some(package) = &job.separator
    {
        match crate::karaoke::coreml::load(package) {
            Ok(loaded) => *separator = Some(loaded),
            Err(error) => log::warn!("words: hearing the whole song: {error:#}"),
        }
    }
    if aligner.is_none() {
        *aligner = Some(coreml::Aligner::load(&job.aligner)?);
    }
    let aligner = aligner.as_mut().expect("loaded above");
    let lines: Vec<(u32, &str)> = job
        .lines
        .iter()
        .map(|(at, text)| (*at, text.as_str()))
        .collect();
    let placed = place(
        &song,
        separator.as_deref_mut(),
        |voice| aligner.emissions(voice),
        &lines,
    )?;
    log::info!(
        "words: placed {} of {} lines in {:?} (decoding took {decoded:?})",
        placed.iter().flatten().count(),
        placed.len(),
        started.elapsed()
    );
    Ok(placed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gives back the window's left channel as the voice, doubled, so the
    /// stitching shows.
    struct Doubler;

    impl Separator for Doubler {
        fn vocals(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
            assert_eq!(audio.len(), crate::karaoke::WINDOW * 2);
            Ok(audio.iter().map(|s| s * 2.0).collect())
        }
    }

    #[test]
    fn a_song_is_separated_whole_from_overlapping_windows() {
        let frames = crate::karaoke::WINDOW * 3 + 12_345;
        let song: Vec<f32> = (0..frames * 2).map(|i| (i % 1000) as f32).collect();
        let voice = separate(&song, &mut Doubler).unwrap();
        assert_eq!(voice.len(), song.len());
        assert!(voice.iter().zip(&song).all(|(v, s)| *v == s * 2.0));
    }

    #[test]
    fn the_voice_sounds_where_it_is_loud() {
        let size = 882;
        let mut voice = vec![0.0; size * 10];
        for sample in &mut voice[size * 3..size * 6] {
            *sample = 0.5;
        }
        let voiced = voiced(&voice);
        assert_eq!(voiced.len(), 10);
        assert_eq!(voiced.iter().filter(|v| **v).count(), 3, "{voiced:?}");
        assert!(voiced[3..6].iter().all(|v| *v));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_playing_song_goes_before_songs_made_ready_ahead() {
        let let_go = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let job = |name: &'static str, ahead: bool| {
            let let_go = std::sync::Arc::clone(&let_go);
            Job {
                track: None,
                lines: Vec::new(),
                ahead,
                aligner: PathBuf::new(),
                separator: None,
                cache: PathBuf::from(name),
                done: Box::new(move |result| {
                    assert!(result.is_err());
                    let_go.lock().unwrap().push(name);
                }),
            }
        };
        let mut pending = vec![
            job("next", true),
            job("playing", false),
            job("later", true),
            job("now", false),
        ];
        assert_eq!(next_job(&mut pending).cache, PathBuf::from("now"));
        assert_eq!(*let_go.lock().unwrap(), ["playing"]);
        assert_eq!(next_job(&mut pending).cache, PathBuf::from("later"));
        assert_eq!(*let_go.lock().unwrap(), ["playing", "next"]);
        assert!(pending.is_empty());
    }

    #[test]
    fn the_cache_is_kept_for_the_lyrics_it_placed() {
        let dir = Path::new("/c");
        let a = cache_path(dir, "spotify:track:abc", &[(0, "hi")]);
        assert!(a.to_string_lossy().starts_with("/c/words-abc-"));
        assert_ne!(a, cache_path(dir, "spotify:track:abc", &[(10, "hi")]));
    }

    /// Places a song's words with the real models: `WORDS_ALIGNER` and
    /// `KARAOKE_MODEL` are their packages, `KARAOKE_SONG` raw stereo `f32`
    /// at 44.1 kHz, and `WORDS_LINES` the lyrics, a line's start in
    /// milliseconds, a tab, then its words.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "needs the models and a song"]
    fn a_real_song_has_its_words_placed() {
        let var = |name: &str| PathBuf::from(std::env::var(name).unwrap());
        let song: Vec<f32> = std::fs::read(var("KARAOKE_SONG"))
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let text = std::fs::read_to_string(var("WORDS_LINES")).unwrap();
        let lines: Vec<(u32, &str)> = text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(at, words)| (at.parse().unwrap(), words))
            .collect();
        let started = std::time::Instant::now();
        let mut separator = crate::karaoke::coreml::load(&var("KARAOKE_MODEL")).unwrap();
        let mut aligner = coreml::Aligner::load(&var("WORDS_ALIGNER")).unwrap();
        let loaded = started.elapsed();
        let placed = place(
            &song,
            Some(separator.as_mut()),
            |voice| aligner.emissions(voice),
            &lines,
        )
        .unwrap();
        eprintln!(
            "loaded in {loaded:?}, placed in {:?}",
            started.elapsed() - loaded
        );
        for ((at, text), words) in lines.iter().zip(&placed) {
            let words = words.as_ref().map(|words| {
                words
                    .iter()
                    .map(|word| {
                        format!(
                            "{}@{}-{}",
                            word.text,
                            word.marks[0],
                            word.marks.last().unwrap()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            eprintln!("{at:>6} {text}\n       {words:?}");
        }
        assert!(placed.iter().flatten().count() * 10 >= placed.len() * 8);
    }

    #[test]
    fn resampling_keeps_the_length_in_time() {
        let out = resample(&vec![0.25; 44_100]);
        assert_eq!(out.len(), 16_000);
        assert!((out[8000] - 0.25).abs() < 1e-3);
    }
}
