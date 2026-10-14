//! Finds when each letter of a song's lyrics is sung.
//!
//! A speech model hears the voice and gives, fifty times a second, how
//! likely each letter is (CTC emissions). The lyrics say which letters come,
//! in order; the best path through the emissions that spells them out in
//! that order places every letter in time. Each line's own time, from the
//! synced lyrics, bounds where its letters may fall, so a repeated chorus
//! cannot be matched to the wrong one.

use serde::{Deserialize, Serialize};

/// How long one emission frame lasts.
pub const FRAME_MS: u32 = 20;

/// The model's alphabet, in the order of its outputs: the CTC blank, the
/// word separator, then the letters by frequency (torchaudio's labels for
/// wav2vec2-base-960h).
pub const LABELS: [char; 29] = [
    '-', '|', 'E', 'T', 'A', 'O', 'N', 'I', 'H', 'S', 'R', 'D', 'L', 'U', 'M', 'W', 'C', 'F', 'G',
    'Y', 'P', 'B', 'V', 'K', '\'', 'X', 'J', 'Q', 'Z',
];
const BLANK: usize = 0;
const SPACE: usize = 1;

/// How far outside its line's time a letter may be sung. Line times mark
/// roughly when a line starts; singers come in early and run late.
const SLACK_MS: u32 = 1000;
/// The longest a word's last letter is held when nothing says otherwise.
const LONGEST_HOLD_MS: u32 = 2000;

/// Log-probabilities of each label, `LABELS.len()` per frame.
pub struct Emissions {
    pub data: Vec<f32>,
}

impl Emissions {
    pub fn frames(&self) -> usize {
        self.data.len() / LABELS.len()
    }

    fn at(&self, frame: usize, label: usize) -> f32 {
        self.data[frame * LABELS.len() + label]
    }
}

/// A word of a lyric line and when it is sung: `marks[i]` is when its
/// `i`th character starts, and the last mark when the word ends, so there
/// is one more mark than characters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub marks: Vec<u32>,
}

/// A letter to place, and the character of the lyrics it spells.
struct Token {
    label: usize,
    line: usize,
    word: usize,
    char: usize,
    /// The frames it may fall on.
    first: usize,
    last: usize,
}

/// The letters a character is heard as: accents dropped, so a song in
/// another language with Latin letters still lines up.
fn letters(c: char) -> &'static str {
    match c.to_ascii_uppercase() {
        'A' => "A",
        'B' => "B",
        'C' => "C",
        'D' => "D",
        'E' => "E",
        'F' => "F",
        'G' => "G",
        'H' => "H",
        'I' => "I",
        'J' => "J",
        'K' => "K",
        'L' => "L",
        'M' => "M",
        'N' => "N",
        'O' => "O",
        'P' => "P",
        'Q' => "Q",
        'R' => "R",
        'S' => "S",
        'T' => "T",
        'U' => "U",
        'V' => "V",
        'W' => "W",
        'X' => "X",
        'Y' => "Y",
        'Z' => "Z",
        '\'' | '\u{2019}' | '\u{2018}' => "'",
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'Ā' | 'ā' | 'Ă'
        | 'ă' | 'Ą' | 'ą' => "A",
        'Æ' | 'æ' => "AE",
        'Ç' | 'ç' | 'Ć' | 'ć' | 'Č' | 'č' => "C",
        'Ď' | 'ď' | 'Đ' | 'đ' | 'Ð' | 'ð' => "D",
        'È' | 'É' | 'Ê' | 'Ë' | 'è' | 'é' | 'ê' | 'ë' | 'Ē' | 'ē' | 'Ė' | 'ė' | 'Ę' | 'ę' | 'Ě'
        | 'ě' => "E",
        'Ğ' | 'ğ' => "G",
        'Ì' | 'Í' | 'Î' | 'Ï' | 'ì' | 'í' | 'î' | 'ï' | 'Ī' | 'ī' | 'İ' | 'ı' => "I",
        'Ł' | 'ł' | 'Ľ' | 'ľ' => "L",
        'Ñ' | 'ñ' | 'Ń' | 'ń' | 'Ň' | 'ň' => "N",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'Ō' | 'ō' | 'Ő'
        | 'ő' => "O",
        'Œ' | 'œ' => "OE",
        'Ř' | 'ř' => "R",
        'Ś' | 'ś' | 'Š' | 'š' | 'Ş' | 'ş' => "S",
        'ß' => "SS",
        'Ť' | 'ť' | 'Ţ' | 'ţ' => "T",
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'ù' | 'ú' | 'û' | 'ü' | 'Ū' | 'ū' | 'Ů' | 'ů' | 'Ű' | 'ű' => {
            "U"
        }
        'Ý' | 'ý' | 'ÿ' | 'Ÿ' => "Y",
        'Ź' | 'ź' | 'Ż' | 'ż' | 'Ž' | 'ž' => "Z",
        _ => "",
    }
}

fn label(letter: char) -> usize {
    LABELS.iter().position(|&l| l == letter).unwrap_or(BLANK)
}

/// When each word of each line is sung. `lines` are the synced lyrics'
/// start times and words; `voiced` says, frame by frame, whether the voice
/// is sounding, which lets a held last note run on. A line whose letters
/// the model can't spell (another script, or nothing but numbers), or that
/// it barely hears, is `None`, and so is every line when the lyrics and the
/// song don't agree at all.
pub fn align(
    emissions: &Emissions,
    lines: &[(u32, &str)],
    voiced: Option<&[bool]>,
) -> Vec<Option<Vec<Word>>> {
    let frames = emissions.frames();
    let tokens = tokens(lines, frames);
    let mut aligned = vec![None; lines.len()];
    let Some(held) = viterbi(emissions, &tokens) else {
        return aligned;
    };
    // Where each line's first letter falls, which ends the line before.
    let mut line_starts = vec![None; lines.len()];
    for (token, &(first, _)) in tokens.iter().zip(&held) {
        if token.label != SPACE {
            line_starts[token.line].get_or_insert(first);
        }
    }
    for (index, (_, text)) in lines.iter().enumerate() {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut starts: Vec<Vec<Option<usize>>> = words
            .iter()
            .map(|word| vec![None; word.chars().count()])
            .collect();
        let mut ends: Vec<Option<usize>> = vec![None; words.len()];
        let mut heard = Vec::new();
        for (token, &(first, last)) in tokens.iter().zip(&held) {
            if token.line != index || token.label == SPACE {
                continue;
            }
            let start = &mut starts[token.word][token.char];
            *start = Some(start.map_or(first, |start| start.min(first)));
            ends[token.word] = Some(ends[token.word].map_or(last + 1, |end| end.max(last + 1)));
            heard.push(
                (first..=last)
                    .map(|frame| emissions.at(frame, token.label))
                    .fold(f32::NEG_INFINITY, f32::max),
            );
        }
        if heard.is_empty() {
            continue;
        }
        let confidence = heard.iter().sum::<f32>() / heard.len() as f32;
        if confidence < MIN_CONFIDENCE {
            log::debug!("words: line {index} barely heard ({confidence:.1}): {text}");
            continue;
        }
        let after = line_starts[index + 1..]
            .iter()
            .flatten()
            .next()
            .copied()
            .unwrap_or(frames);
        aligned[index] = Some(lay_out(&words, &starts, &ends, after, voiced));
    }
    aligned
}

/// A line's letters are, on average, at least this likely where the path
/// puts them (a natural log), or the line is not what is being sung.
const MIN_CONFIDENCE: f32 = -6.0;

/// Words with a mark for every character, from the frames their letters
/// start and end on. Characters with no letter of their own (punctuation,
/// digits) and words with none at all are spread between their neighbours.
/// `after` is where the next line's singing starts.
fn lay_out(
    words: &[&str],
    starts: &[Vec<Option<usize>>],
    ends: &[Option<usize>],
    after: usize,
    voiced: Option<&[bool]>,
) -> Vec<Word> {
    let hold = (LONGEST_HOLD_MS / FRAME_MS) as usize;
    let first_heard = |word: usize| starts[word].iter().flatten().min().copied();
    // Each heard word's start and end, the end running on while the voice
    // does, up to the next word.
    let mut spans: Vec<Option<(usize, usize)>> = vec![None; words.len()];
    for word in 0..words.len() {
        let (Some(start), Some(end)) = (first_heard(word), ends[word]) else {
            continue;
        };
        let next = (word + 1..words.len())
            .find_map(first_heard)
            .unwrap_or(after);
        let limit = next.min(end + hold).max(end);
        let end = match voiced {
            Some(voiced) => {
                let mut end = end;
                while end < limit && voiced.get(end).copied().unwrap_or(false) {
                    end += 1;
                }
                end
            }
            // Without the voice to go by, a word runs into a close next one.
            None if next.saturating_sub(end) <= 25 => next.max(end),
            None => end,
        };
        spans[word] = Some((start, end));
    }
    // Words the model couldn't spell share the time between their
    // neighbours.
    let mut word = 0;
    while word < words.len() {
        if spans[word].is_some() {
            word += 1;
            continue;
        }
        let gap_end = (word..words.len())
            .find(|&w| spans[w].is_some())
            .unwrap_or(words.len());
        let from = word
            .checked_sub(1)
            .and_then(|w| spans[w])
            .map_or_else(|| spans[gap_end].map_or(0, |s| s.0), |s| s.1);
        let to = spans
            .get(gap_end)
            .copied()
            .flatten()
            .map_or(from.max(after.min(from + 25)), |s| s.0)
            .max(from);
        let count = gap_end - word;
        for (i, slot) in spans[word..gap_end].iter_mut().enumerate() {
            let at = |k: usize| from + (to - from) * k / count;
            *slot = Some((at(i), at(i + 1)));
        }
        word = gap_end;
    }
    words
        .iter()
        .zip(spans)
        .enumerate()
        .map(|(word, (text, span))| {
            let (start, end) = span.unwrap_or_default();
            let count = starts[word].len();
            // Known marks, then the unknown ones spread between them.
            let mut marks: Vec<Option<usize>> = starts[word].clone();
            marks.push(Some(end));
            if marks[0].is_none() {
                marks[0] = Some(start);
            }
            let mut i = 1;
            while i < count {
                if marks[i].is_some() {
                    i += 1;
                    continue;
                }
                let next = (i..=count).find(|&k| marks[k].is_some()).unwrap_or(count);
                let (a, b) = (marks[i - 1].unwrap_or(start), marks[next].unwrap_or(end));
                let span = next - (i - 1);
                for (k, mark) in marks.iter_mut().enumerate().take(next).skip(i) {
                    *mark = Some(a + (b.max(a) - a) * (k - (i - 1)) / span);
                }
                i = next;
            }
            let mut floor = 0;
            let marks = marks
                .into_iter()
                .map(|mark| {
                    floor = floor.max(mark.unwrap_or(floor));
                    floor as u32 * FRAME_MS
                })
                .collect();
            Word {
                text: (*text).to_string(),
                marks,
            }
        })
        .collect()
}

/// Lays out the tokens of `lines` with the frames each may fall on.
fn tokens(lines: &[(u32, &str)], frames: usize) -> Vec<Token> {
    let frame = |ms: u32| (ms / FRAME_MS) as usize;
    let mut tokens: Vec<Token> = Vec::new();
    for (index, (at, text)) in lines.iter().enumerate() {
        // A line may run until the next one starts.
        let until = lines
            .get(index + 1)
            .map_or(frames, |(next, _)| frame(next.saturating_add(SLACK_MS)));
        let first = frame(at.saturating_sub(SLACK_MS));
        let last = until.min(frames).saturating_sub(1).max(first);
        for (word, text) in text.split_whitespace().enumerate() {
            let before = tokens.len();
            for (char, c) in text.chars().enumerate() {
                for letter in letters(c).chars() {
                    tokens.push(Token {
                        label: label(letter),
                        line: index,
                        word,
                        char,
                        first,
                        last,
                    });
                }
            }
            // Words are kept apart by the separator, which the model hears
            // between them.
            if tokens.len() > before && before > 0 {
                let previous = &tokens[before - 1];
                let separator = Token {
                    label: SPACE,
                    line: index,
                    word,
                    char: usize::MAX,
                    first: previous.first.min(first),
                    last,
                };
                tokens.insert(before, separator);
            }
        }
    }
    // Later letters can't be allowed earlier than earlier ones.
    let mut floor = 0;
    for token in &mut tokens {
        floor = floor.max(token.first);
        token.first = floor;
    }
    let mut ceiling = usize::MAX;
    for token in tokens.iter_mut().rev() {
        ceiling = ceiling.min(token.last);
        token.last = ceiling;
    }
    tokens
}

/// The best path through the emissions spelling out `tokens`, as the first
/// and last frame each token holds, or `None` when no path keeps every
/// token to its frames.
fn viterbi(emissions: &Emissions, tokens: &[Token]) -> Option<Vec<(usize, usize)>> {
    let frames = emissions.frames();
    if tokens.is_empty() || frames == 0 {
        return None;
    }
    // States alternate blank, token, blank, …: state 2k + 1 is token k.
    let states = 2 * tokens.len() + 1;
    let token = |state: usize| (state % 2 == 1).then(|| &tokens[state / 2]);
    let label = |state: usize| token(state).map_or(BLANK, |token| token.label);
    let allowed = |state: usize, frame: usize| {
        token(state).is_none_or(|token| (token.first..=token.last).contains(&frame))
    };
    // The states reachable at each frame form a window; only it is kept.
    let mut windows = Vec::with_capacity(frames);
    let mut low = 0usize;
    let mut high = 0usize;
    for frame in 0..frames {
        // Every token that must be over by now has been passed.
        while low / 2 < tokens.len() && tokens[low / 2].last < frame {
            low = 2 * (low / 2) + 2;
        }
        // Tokens whose time has come may have been reached.
        while high + 1 < states
            && token(high + 1).is_none_or(|token| token.first <= frame)
            && high < 2 * frame + 1
        {
            high += 1;
        }
        if low > high {
            return None;
        }
        windows.push((low, high));
    }
    let mut score = vec![f32::NEG_INFINITY; states];
    let mut next = vec![f32::NEG_INFINITY; states];
    // How each state was entered, by frame: 0 stayed, 1 or 2 came from that
    // many states back.
    let mut steps: Vec<Vec<u8>> = Vec::with_capacity(frames);
    let (low, high) = windows[0];
    for (state, score) in score.iter_mut().enumerate().take(high.min(1) + 1).skip(low) {
        if allowed(state, 0) {
            *score = emissions.at(0, label(state));
        }
    }
    steps.push(vec![0u8; high - low + 1]);
    for frame in 1..frames {
        let (low, high) = windows[frame];
        let (before_low, before_high) = windows[frame - 1];
        let mut step = vec![0u8; high - low + 1];
        for state in low..=high {
            next[state] = f32::NEG_INFINITY;
            if !allowed(state, frame) {
                continue;
            }
            let from = |back: usize| {
                state
                    .checked_sub(back)
                    .filter(|&from| (before_low..=before_high).contains(&from))
                    .map_or(f32::NEG_INFINITY, |from| score[from])
            };
            let mut best = (from(0), 0u8);
            if from(1) > best.0 {
                best = (from(1), 1);
            }
            if state % 2 == 1 && state >= 2 && label(state) != label(state - 2) && from(2) > best.0
            {
                best = (from(2), 2);
            }
            if best.0 > f32::NEG_INFINITY {
                next[state] = best.0 + emissions.at(frame, label(state));
                step[state - low] = best.1;
            }
        }
        // States the window has left behind can't be come from again.
        score[before_low..low.max(before_low)].fill(f32::NEG_INFINITY);
        score[low..=high].copy_from_slice(&next[low..=high]);
        steps.push(step);
    }
    // The path ends on the last token or the blank after it.
    let (low, high) = windows[frames - 1];
    let mut state = [states - 1, states - 2]
        .into_iter()
        .filter(|state| (low..=high).contains(state))
        .max_by(|a, b| score[*a].total_cmp(&score[*b]))?;
    if score[state] == f32::NEG_INFINITY {
        return None;
    }
    let mut held = vec![(usize::MAX, 0usize); tokens.len()];
    for frame in (0..frames).rev() {
        if state % 2 == 1 {
            let span = &mut held[state / 2];
            span.0 = frame;
            if span.1 < frame {
                span.1 = frame;
            }
        }
        let (low, _) = windows[frame];
        state -= steps[frame][state - low] as usize;
    }
    Some(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Emissions that say, frame by frame, which label is heard.
    fn heard(labels: &[usize]) -> Emissions {
        let mut data = vec![-20.0; labels.len() * LABELS.len()];
        for (frame, &label) in labels.iter().enumerate() {
            data[frame * LABELS.len() + label] = -0.01;
        }
        Emissions { data }
    }

    fn spoken(text: &str, gap: usize) -> Vec<usize> {
        let mut frames = vec![BLANK; gap];
        for c in text.chars() {
            frames.push(if c == ' ' { SPACE } else { label(c) });
            frames.extend(std::iter::repeat_n(BLANK, gap));
        }
        frames
    }

    #[test]
    fn letters_lose_their_accents_and_keep_the_apostrophe() {
        let spelled: String = "Ça n’est été Straße 2".chars().map(letters).collect();
        assert_eq!(spelled, "CAN'ESTETESTRASSE");
    }

    #[test]
    fn the_path_finds_each_letter_where_it_is_heard() {
        let emissions = heard(&spoken("HI YO", 3));
        let tokens = tokens(&[(0, "hi yo")], emissions.frames());
        assert_eq!(tokens.len(), 5, "four letters and a separator");
        let held = viterbi(&emissions, &tokens).unwrap();
        let starts: Vec<usize> = held.iter().map(|span| span.0).collect();
        assert_eq!(starts, [3, 7, 11, 15, 19]);
    }

    #[test]
    fn a_double_letter_needs_a_blank_between() {
        let emissions = heard(&spoken("ALL", 2));
        let tokens = tokens(&[(0, "all")], emissions.frames());
        let held = viterbi(&emissions, &tokens).unwrap();
        assert_eq!(held.iter().map(|s| s.0).collect::<Vec<_>>(), [2, 5, 8]);
    }

    #[test]
    fn a_repeated_line_matches_the_one_sung_at_its_time() {
        // The same words twice, ten seconds apart; the second line's time
        // keeps it from taking the first one's singing.
        let mut frames = spoken("GO", 5);
        frames.extend(std::iter::repeat_n(BLANK, 500));
        let second = frames.len();
        frames.extend(spoken("GO", 5));
        let emissions = heard(&frames);
        // Only the second is in the lyrics: its time keeps it from taking
        // the first one's singing, which spells it out just as well.
        let lines = [(second as u32 * FRAME_MS, "go")];
        let tokens = tokens(&lines, emissions.frames());
        let held = viterbi(&emissions, &tokens).unwrap();
        assert_eq!(held[0].0, second + 5);
        assert_eq!(held[1].0, second + 11);
    }

    #[test]
    fn words_are_marked_letter_by_letter_and_run_on_while_the_voice_does() {
        // "Oh, go!" with the O of "go" held for a second.
        let mut frames = spoken("OH", 2);
        frames.push(SPACE);
        frames.extend([BLANK, BLANK, label('G'), BLANK, label('O')]);
        let held_from = frames.len();
        frames.extend(std::iter::repeat_n(BLANK, 60));
        let emissions = heard(&frames);
        let voiced: Vec<bool> = (0..frames.len()).map(|f| f < held_from + 50).collect();
        let aligned = align(&emissions, &[(0, "Oh, go!")], Some(&voiced));
        let words = aligned[0].as_ref().unwrap();
        assert_eq!(words[0].text, "Oh,");
        assert_eq!(words[0].marks.len(), 4, "a mark per character and the end");
        assert_eq!(words[0].marks[..2], [40, 100]);
        assert_eq!(
            words[0].marks[3],
            11 * FRAME_MS,
            "sounding on, it runs into the next word"
        );
        let go = &words[1].marks;
        assert_eq!(go[..2], [11 * FRAME_MS, 13 * FRAME_MS]);
        assert_eq!(
            *go.last().unwrap(),
            (held_from + 50) as u32 * FRAME_MS,
            "held"
        );
    }

    #[test]
    fn a_word_with_nothing_to_hear_shares_its_neighbours_time() {
        let emissions = heard(&spoken("AB C", 4));
        let aligned = align(&emissions, &[(0, "ab 22 c")], None);
        let words = aligned[0].as_ref().unwrap();
        let (ab, number, c) = (&words[0].marks, &words[1].marks, &words[2].marks);
        assert!(ab.last() <= number.first());
        assert!(number.last() <= c.first());
        assert_eq!(c[0], 19 * FRAME_MS);
    }

    #[test]
    fn a_line_in_another_script_has_no_words() {
        let emissions = heard(&spoken("LA", 3));
        let aligned = align(&emissions, &[(0, "la"), (40, "你好")], None);
        assert!(aligned[0].is_some());
        assert!(aligned[1].is_none());
    }

    #[test]
    fn letters_that_cannot_fit_their_time_find_no_path() {
        let emissions = heard(&spoken("A", 1));
        let tokens = tokens(&[(0, "abcdefgh")], emissions.frames());
        assert!(viterbi(&emissions, &tokens).is_none());
    }
}

#[cfg(test)]
mod real {
    use super::*;

    /// Aligns lyrics against emissions the real model gave for a song.
    /// `WORDS_EMISSIONS` is raw little-endian `f32`, 29 per frame;
    /// `WORDS_LINES` has a line per lyric line, its start in milliseconds,
    /// a tab, then its words.
    #[test]
    #[ignore = "needs the model's emissions for a song"]
    fn real_emissions_place_the_words() {
        let data = std::fs::read(std::env::var("WORDS_EMISSIONS").unwrap())
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let text = std::fs::read_to_string(std::env::var("WORDS_LINES").unwrap()).unwrap();
        let lines: Vec<(u32, &str)> = text
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(at, words)| (at.parse().unwrap(), words))
            .collect();
        let started = std::time::Instant::now();
        let aligned = align(&Emissions { data }, &lines, None);
        eprintln!("aligned in {:?}", started.elapsed());
        for ((at, text), words) in lines.iter().zip(&aligned) {
            let words = words.as_ref().map(|words| {
                words
                    .iter()
                    .map(|word| format!("{}@{}", word.text, word.marks[0]))
                    .collect::<Vec<_>>()
                    .join(" ")
            });
            eprintln!("{at:>6} {text}\n       {words:?}");
        }
        // A song's lines are nearly all placed; one cut off at its end is not.
        assert!(aligned.iter().flatten().count() * 10 >= aligned.len() * 8);
    }
}
