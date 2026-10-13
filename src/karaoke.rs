//! Karaoke: takes the singing out of local playback.
//!
//! A vocal separation model hears the decoded mix, and the stage here plays
//! the mix less as much of the voice as the listener asks. The model works on
//! windows of almost six seconds and needs a little of what comes next, so
//! the stage holds about a second of sound: the decoder runs that far ahead
//! of what is heard, which the player takes off the positions it reports.
//!
//! The model runs on a thread of its own. The player's thread only copies
//! sound in and out, and when the model falls behind it plays the mix as it
//! is rather than wait.

mod mdx;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use anyhow::Result;
use librespot_playback::SAMPLE_RATE;

pub use mdx::{Mdx, Model};

/// Samples between the model's analysis frames.
pub const HOP: usize = 1024;
/// Analysis frames in one model window.
pub const FRAMES: usize = 256;
/// Stereo frames in one model window, about 5.9 seconds.
pub const WINDOW: usize = HOP * (FRAMES - 1);
/// How much of what follows a stretch the model hears before that stretch
/// is played. Measured on real songs, a quarter of a second separates as
/// well as the whole of the window would.
const LOOKAHEAD: usize = SAMPLE_RATE as usize / 4;
/// The stretch each model run produces. Longer runs less often, for the
/// same work per second of music; shorter holds less sound.
const STEP: usize = SAMPLE_RATE as usize * 3 / 4;
/// Neighbouring runs disagree a little; their seam is blended this long.
const FADE: usize = 1024;
/// How long removal takes to come in or go out, in frames.
const RAMP: usize = 4096;
/// The most the stage holds before it waits for the model.
const LIMIT: usize = LOOKAHEAD + 2 * STEP;
/// How long the player's thread waits for a late run before it plays that
/// stretch as it is. Playing along, the model is never late: it runs in a
/// tenth of the time a stretch takes to play. It is waited for just after a
/// seek, when the decoder has raced ahead and nothing is queued to play, so
/// this is the most a seek can be held up. A model still loading is not
/// waited for at all.
const PATIENCE: Duration = Duration::from_millis(250);

/// Separates the voice from a window of the mix.
pub trait Separator: Send {
    /// The vocals of [`WINDOW`] interleaved stereo frames, in the same form.
    fn vocals(&mut self, audio: &[f32]) -> Result<Vec<f32>>;
}

/// Makes the separator, on the thread that will run it.
pub type Loader = Arc<dyn Fn() -> Result<Box<dyn Separator>> + Send + Sync>;

/// Where the model stands, for the interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    /// Not asked for, or no model yet.
    #[default]
    Off,
    /// The model is loading.
    Preparing,
    /// The voice is being taken out.
    Running,
    /// The model could not be loaded or run.
    Failed,
}

impl Status {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Preparing,
            2 => Self::Running,
            3 => Self::Failed,
            _ => Self::Off,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Preparing => "preparing",
            Self::Running => "running",
            Self::Failed => "failed",
        }
    }
}

/// Karaoke as the app and the player's thread share it.
pub struct Karaoke {
    enabled: AtomicBool,
    /// How much of the voice is taken out, from 0 to 1, as `f32` bits.
    removal: AtomicU32,
    loader: Mutex<Option<Loader>>,
    status: AtomicU8,
    /// Frames the stage holds back, which the player takes off positions.
    held: AtomicU64,
    /// The decoder reached the end of a track and nothing has followed yet:
    /// a stop now ends the music, so what is held is played out first.
    ended: AtomicBool,
    /// Moves on at every reset, so the model skips windows from before it.
    generation: AtomicU64,
}

pub type SharedKaraoke = Arc<Karaoke>;

impl Default for Karaoke {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            removal: AtomicU32::new(1.0f32.to_bits()),
            loader: Mutex::new(None),
            status: AtomicU8::new(Status::Off as u8),
            held: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }
}

impl Karaoke {
    pub fn shared() -> SharedKaraoke {
        Arc::new(Self::default())
    }

    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Sets how much of the voice is left, from 0 (none) to 1 (all of it).
    pub fn set_vocal_level(&self, level: f32) {
        let removal = 1.0 - level.clamp(0.0, 1.0);
        self.removal.store(removal.to_bits(), Ordering::Relaxed);
    }

    fn removal(&self) -> f32 {
        f32::from_bits(self.removal.load(Ordering::Relaxed))
    }

    /// Gives the stage a way to make its model, or takes it away.
    pub fn set_loader(&self, loader: Option<Loader>) {
        *self.loader.lock().unwrap_or_else(PoisonError::into_inner) = loader;
    }

    fn loader(&self) -> Option<Loader> {
        self.loader
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn has_model(&self) -> bool {
        self.loader().is_some()
    }

    pub fn status(&self) -> Status {
        Status::from_u8(self.status.load(Ordering::Relaxed))
    }

    fn set_status(&self, status: Status) {
        self.status.store(status as u8, Ordering::Relaxed);
    }

    /// How far behind the decoder the listener is, because of the stage.
    pub fn lag(&self) -> Duration {
        let frames = self.held.load(Ordering::Relaxed);
        Duration::from_secs_f64(frames as f64 / f64::from(SAMPLE_RATE))
    }

    /// The decoder finished a track; see [`Karaoke::ended`].
    pub fn track_ended(&self) {
        self.ended.store(true, Ordering::Relaxed);
    }

    fn take_ended(&self) -> bool {
        self.ended.swap(false, Ordering::Relaxed)
    }

    /// Whether the stage should be taking the voice out.
    fn wanted(&self) -> bool {
        self.enabled() && self.has_model()
    }
}

/// A window for the model, and the stretch of it to send back.
struct Job {
    generation: u64,
    /// The stretch's first frame.
    start: u64,
    audio: Vec<f32>,
}

/// The vocals of a stretch and the [`FADE`] frames after it, or nothing
/// when the model could not run.
struct Done {
    generation: u64,
    start: u64,
    vocals: Option<Vec<f32>>,
}

/// The model's thread.
struct Worker {
    jobs: Sender<Job>,
    done: Receiver<Done>,
}

impl Worker {
    fn spawn(control: &SharedKaraoke) -> Option<Self> {
        let loader = control.loader()?;
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (outbox, done) = mpsc::channel();
        let control = Arc::clone(control);
        control.set_status(Status::Preparing);
        let spawned = std::thread::Builder::new()
            .name("karaoke".into())
            .spawn(move || {
                let mut separator = match loader() {
                    Ok(separator) => Some(separator),
                    Err(error) => {
                        log::error!("karaoke: the vocal model did not load: {error:#}");
                        control.set_status(Status::Failed);
                        None
                    }
                };
                if separator.is_some() {
                    control.set_status(Status::Running);
                }
                for job in inbox {
                    // A reset since this was asked for: nobody wants it.
                    if job.generation != control.generation.load(Ordering::Relaxed) {
                        continue;
                    }
                    let vocals = separator.as_mut().and_then(|separator| {
                        match separator.vocals(&job.audio) {
                            Ok(vocals) => Some(stretch(&vocals)),
                            Err(error) => {
                                log::error!("karaoke: the vocal model failed: {error:#}");
                                control.set_status(Status::Failed);
                                None
                            }
                        }
                    });
                    let done = Done {
                        generation: job.generation,
                        start: job.start,
                        vocals,
                    };
                    if outbox.send(done).is_err() {
                        break;
                    }
                }
            });
        match spawned {
            Ok(_) => Some(Self { jobs, done }),
            Err(error) => {
                log::error!("karaoke: no thread for the vocal model: {error}");
                None
            }
        }
    }
}

/// The part of a window's vocals its job is for: the stretch that ends
/// [`LOOKAHEAD`] before the window does, and the [`FADE`] after it.
fn stretch(vocals: &[f32]) -> Vec<f32> {
    let start = WINDOW - LOOKAHEAD - STEP;
    vocals[start * 2..(start + STEP + FADE) * 2].to_vec()
}

/// The stage on the player's thread. Frame numbers count from the last
/// reset.
pub struct Stage {
    control: SharedKaraoke,
    worker: Option<Worker>,
    /// Interleaved frames from `base` up to `received`.
    input: VecDeque<f32>,
    base: u64,
    received: u64,
    /// Frames played out.
    emitted: u64,
    /// Vocals for the frames from `emitted` up to `ready`.
    vocals: VecDeque<f32>,
    ready: u64,
    /// The end of the last stretch sent to the model.
    asked: u64,
    /// The last run's idea of the [`FADE`] frames after its stretch.
    tail: Option<Vec<f32>>,
    /// How much of the voice is being taken out now, moving to the setting.
    removal: f32,
}

impl Stage {
    pub fn new(control: SharedKaraoke) -> Self {
        Self {
            control,
            worker: None,
            input: VecDeque::new(),
            base: 0,
            received: 0,
            emitted: 0,
            vocals: VecDeque::new(),
            ready: 0,
            asked: 0,
            tail: None,
            removal: 0.0,
        }
    }

    /// Whether sound is held back.
    pub fn holding(&self) -> bool {
        self.received > self.emitted
    }

    /// Forgets what is held, after a seek or a skip. The model stays loaded.
    pub fn reset(&mut self) {
        self.control.generation.fetch_add(1, Ordering::Relaxed);
        self.input.clear();
        self.vocals.clear();
        self.base = 0;
        self.received = 0;
        self.emitted = 0;
        self.ready = 0;
        self.asked = 0;
        self.tail = None;
        self.publish();
    }

    /// Takes in interleaved stereo samples and gives back what is ready to
    /// be heard, which can be more or less than came in.
    pub fn process(&mut self, samples: Vec<f64>) -> Vec<f64> {
        if !self.control.wanted() {
            if self.worker.is_none() {
                return samples;
            }
            // Turned off: what is held goes out with the voice coming back,
            // then the sound passes straight through.
            let mut out = self.drain(true);
            self.worker = None;
            self.control.set_status(Status::Off);
            self.reset();
            out.extend(samples);
            return out;
        }
        if self.worker.is_none() {
            self.worker = Worker::spawn(&self.control);
            if self.worker.is_none() {
                return samples;
            }
            self.reset();
            // The voice fades out from here rather than cutting off.
            self.removal = 0.0;
        }
        self.control.take_ended();
        self.take_in(&samples);
        self.ask();
        self.collect(false);
        while self.received - self.ready > LIMIT as u64 && self.ready < self.asked {
            if !self.wait() {
                self.give_up_one();
            }
        }
        let out = self.give_out(self.ready.min(self.received), false);
        self.publish();
        out
    }

    /// Plays out everything held when the decoder has reached the end of
    /// the music, so its last second is heard. Nothing happens on a pause.
    pub fn finish(&mut self) -> Vec<f64> {
        if self.control.take_ended() && self.worker.is_some() && self.holding() {
            let out = self.drain(false);
            self.reset();
            return out;
        }
        Vec::new()
    }

    /// Runs the model over the rest of what is held and gives all of it.
    fn drain(&mut self, fading: bool) -> Vec<f64> {
        while self.asked < self.received {
            self.send(self.asked + STEP as u64);
        }
        while self.ready < self.asked {
            if !self.wait() {
                self.give_up_one();
            }
        }
        let out = self.give_out(self.received, fading);
        self.publish();
        out
    }

    fn take_in(&mut self, samples: &[f64]) {
        self.input.extend(samples.iter().map(|&s| s as f32));
        self.received += (samples.len() / 2) as u64;
    }

    /// Sends every stretch whose lookahead has arrived.
    fn ask(&mut self) {
        while self.asked + (STEP + LOOKAHEAD) as u64 <= self.received {
            self.send(self.asked + STEP as u64);
        }
    }

    /// Sends the window whose stretch ends at `end`. Frames before the
    /// first or not yet decoded are silence.
    fn send(&mut self, end: u64) {
        let first = end as i64 + LOOKAHEAD as i64 - WINDOW as i64;
        let mut audio = vec![0.0; WINDOW * 2];
        for (i, slot) in audio.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let frame = first + i as i64;
            if frame >= self.base as i64 && (frame as u64) < self.received {
                let at = (frame as u64 - self.base) as usize * 2;
                *slot = [self.input[at], self.input[at + 1]];
            }
        }
        let job = Job {
            generation: self.control.generation.load(Ordering::Relaxed),
            start: end - STEP as u64,
            audio,
        };
        self.asked = end;
        if let Some(worker) = &self.worker
            && worker.jobs.send(job).is_err()
        {
            log::error!("karaoke: the vocal model's thread has gone");
        }
    }

    /// Takes what the model has finished. With `block`, waits a while for
    /// the next run; returns whether it arrived.
    fn collect(&mut self, block: bool) -> bool {
        let mut got = false;
        loop {
            let Some(worker) = &self.worker else {
                return got;
            };
            let done = if block && !got {
                if self.control.status() != Status::Running {
                    return false;
                }
                match worker.done.recv_timeout(PATIENCE) {
                    Ok(done) => done,
                    Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                        return false;
                    }
                }
            } else {
                match worker.done.try_recv() {
                    Ok(done) => done,
                    Err(_) => return got,
                }
            };
            got |= self.apply(done);
        }
    }

    fn wait(&mut self) -> bool {
        self.collect(true)
    }

    /// Adds a finished run's vocals, if they are still wanted.
    fn apply(&mut self, done: Done) -> bool {
        if done.generation != self.control.generation.load(Ordering::Relaxed)
            || done.start != self.ready
        {
            return false;
        }
        match done.vocals {
            Some(mut vocals) => {
                if let Some(tail) = self.tail.take() {
                    for i in 0..FADE {
                        let w = (i as f32 + 0.5) / FADE as f32;
                        for c in 0..2 {
                            let at = i * 2 + c;
                            vocals[at] = tail[at] * (1.0 - w) + vocals[at] * w;
                        }
                    }
                }
                self.tail = Some(vocals.split_off(STEP * 2));
                self.vocals.extend(vocals);
            }
            None => {
                self.tail = None;
                self.vocals.extend(std::iter::repeat_n(0.0, STEP * 2));
            }
        }
        self.ready += STEP as u64;
        true
    }

    /// The model is too slow for the stretch next in line: it is played as
    /// it is, and the run for it is ignored when it comes.
    fn give_up_one(&mut self) {
        log::warn!("karaoke: the vocal model fell behind; playing a stretch with its voice");
        self.tail = None;
        self.vocals.extend(std::iter::repeat_n(0.0, STEP * 2));
        self.ready += STEP as u64;
    }

    /// Plays the frames up to `until`, less the voice.
    fn give_out(&mut self, until: u64, fading: bool) -> Vec<f64> {
        let count = until.saturating_sub(self.emitted) as usize;
        let target = if fading { 0.0 } else { self.control.removal() };
        let step = 1.0 / RAMP as f32;
        let mut out = Vec::with_capacity(count * 2);
        let offset = (self.emitted - self.base) as usize * 2;
        for i in 0..count {
            self.removal += (target - self.removal).clamp(-step, step);
            for c in 0..2 {
                let mix = self.input[offset + i * 2 + c];
                let voice = self.vocals.get(i * 2 + c).copied().unwrap_or(0.0);
                out.push(f64::from(mix - self.removal * voice));
            }
        }
        self.vocals.drain(..(count * 2).min(self.vocals.len()));
        self.emitted += count as u64;
        // Keep what the next window reaches back for, and what is unplayed.
        let next = (self.asked + (STEP + LOOKAHEAD) as u64).saturating_sub(WINDOW as u64);
        let keep_from = next.min(self.emitted).max(self.base);
        let drop = (keep_from - self.base) as usize * 2;
        self.input.drain(..drop);
        self.base = keep_from;
        out
    }

    fn publish(&self) {
        self.control
            .held
            .store(self.received - self.emitted, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Calls half of everything the voice.
    struct Half;

    impl Separator for Half {
        fn vocals(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
            Ok(audio.iter().map(|s| s * 0.5).collect())
        }
    }

    /// Takes longer than the stage will wait.
    struct Slow;

    impl Separator for Slow {
        fn vocals(&mut self, audio: &[f32]) -> Result<Vec<f32>> {
            std::thread::sleep(PATIENCE * 3);
            Ok(audio.to_vec())
        }
    }

    fn control(separator: fn() -> Box<dyn Separator>) -> SharedKaraoke {
        let control = Karaoke::shared();
        control.set_loader(Some(Arc::new(move || Ok(separator()))));
        control.set_enabled(true);
        control
    }

    /// A ramp of distinct values, so a frame out of place shows.
    fn music(from: usize, frames: usize) -> Vec<f64> {
        (from..from + frames)
            .flat_map(|n| {
                let v = ((n % 1000) as f64 / 1000.0) - 0.5;
                [v, -v]
            })
            .collect()
    }

    /// Equal but for the stage's single precision.
    fn assert_close(heard: &[f64], played: &[f64]) {
        assert_eq!(heard.len(), played.len());
        for (i, (a, b)) in heard.iter().zip(played).enumerate() {
            assert!((a - b).abs() < 1e-6, "sample {i}: {a} for {b}");
        }
    }

    /// Feeds `total` frames in packets, as librespot does, and gathers
    /// what comes out.
    fn play(stage: &mut Stage, total: usize) -> Vec<f64> {
        let mut out = Vec::new();
        let packet = 1_152;
        let mut fed = 0;
        while fed < total {
            let frames = packet.min(total - fed);
            out.extend(stage.process(music(fed, frames)));
            fed += frames;
            // The model loads before the music goes on, as a real one
            // would in the time the first packets take to play.
            while stage.control.status() == Status::Preparing {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        out
    }

    #[test]
    fn the_voice_comes_out_in_time_with_the_music() {
        let control = control(|| Box::new(Half));
        let mut stage = Stage::new(Arc::clone(&control));
        let total = SAMPLE_RATE as usize * 10;
        let mut out = play(&mut stage, total);
        assert!(stage.holding());
        assert!(control.lag() > Duration::from_millis(200));
        assert!(control.lag() < Duration::from_secs(2));
        control.track_ended();
        out.extend(stage.finish());
        assert!(!stage.holding());
        assert_eq!(control.lag(), Duration::ZERO);
        let input = music(0, total);
        assert_eq!(out.len(), input.len(), "every frame is played once");
        // Past the voice's fade in, each frame is the music less half.
        for (i, (heard, played)) in out.iter().zip(&input).enumerate().skip(RAMP * 2 + 2) {
            assert!(
                (heard - played * 0.5).abs() < 1e-4,
                "sample {i}: {heard} for {played}"
            );
        }
    }

    #[test]
    fn a_pause_keeps_what_is_held() {
        let control = control(|| Box::new(Half));
        let mut stage = Stage::new(Arc::clone(&control));
        play(&mut stage, SAMPLE_RATE as usize * 3);
        assert!(stage.finish().is_empty());
        assert!(stage.holding());
    }

    #[test]
    fn a_seek_forgets_what_is_held() {
        let control = control(|| Box::new(Half));
        let mut stage = Stage::new(Arc::clone(&control));
        play(&mut stage, SAMPLE_RATE as usize * 3);
        stage.reset();
        assert!(!stage.holding());
        assert_eq!(control.lag(), Duration::ZERO);
        let out = play(&mut stage, SAMPLE_RATE as usize * 3);
        // The new position's sound, from its first frame.
        let input = music(0, SAMPLE_RATE as usize * 3);
        assert!((out[RAMP * 4] - input[RAMP * 4] * 0.5).abs() < 1e-4);
    }

    #[test]
    fn turning_it_off_plays_out_what_is_held_and_then_passes_through() {
        let control = control(|| Box::new(Half));
        let mut stage = Stage::new(Arc::clone(&control));
        let first = SAMPLE_RATE as usize * 3;
        let mut out = play(&mut stage, first);
        control.set_enabled(false);
        let next = music(first, 1_152);
        out.extend(stage.process(next.clone()));
        assert!(!stage.holding());
        assert_eq!(out.len(), (first + 1_152) * 2);
        // The voice is back by the end of what was held.
        assert_eq!(&out[out.len() - next.len()..], &next[..]);
        assert_eq!(control.status(), Status::Off);
        // And sound passes straight through after.
        let after = music(first + 1_152, 500);
        assert_eq!(stage.process(after.clone()), after);
    }

    #[test]
    fn without_a_model_the_sound_passes_straight_through() {
        let control = Karaoke::shared();
        control.set_enabled(true);
        let mut stage = Stage::new(control);
        let packet = music(0, 1_152);
        assert_eq!(stage.process(packet.clone()), packet);
    }

    #[test]
    fn a_model_that_falls_behind_never_holds_the_music_up() {
        let control = control(|| Box::new(Slow));
        let mut stage = Stage::new(Arc::clone(&control));
        let total = SAMPLE_RATE as usize * 6;
        let started = std::time::Instant::now();
        let out = play(&mut stage, total);
        // Each slow stretch is waited for once at most.
        let stretches = total / STEP + 1;
        assert!(started.elapsed() < PATIENCE * 2 * stretches as u32);
        assert!(out.len() / 2 >= total - LIMIT - STEP);
        // What the slow model never finished is played as it was.
        let input = music(0, total);
        assert_close(&out[..STEP * 2], &input[..STEP * 2]);
    }

    #[test]
    fn a_model_that_fails_to_load_leaves_the_music_alone() {
        let control = Karaoke::shared();
        control.set_loader(Some(Arc::new(|| Err(anyhow::anyhow!("no model")))));
        control.set_enabled(true);
        let mut stage = Stage::new(Arc::clone(&control));
        let total = SAMPLE_RATE as usize * 3;
        let mut out = play(&mut stage, total);
        control.track_ended();
        out.extend(stage.finish());
        assert_close(&out, &music(0, total));
        assert_eq!(control.status(), Status::Failed);
    }

    #[test]
    fn the_level_leaves_that_much_of_the_voice() {
        let control = control(|| Box::new(Half));
        control.set_vocal_level(0.5);
        let mut stage = Stage::new(Arc::clone(&control));
        let total = SAMPLE_RATE as usize * 4;
        let mut out = play(&mut stage, total);
        control.track_ended();
        out.extend(stage.finish());
        let input = music(0, total);
        // Half the voice out: the music less a quarter.
        let i = RAMP * 4;
        assert!((out[i] - input[i] * 0.75).abs() < 1e-4);
    }
}
