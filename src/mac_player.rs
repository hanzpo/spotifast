//! The player capsule as Music draws it: Liquid Glass over the page, with
//! AppKit's own buttons, SF Symbols and sliders on it.
//!
//! The glass has to sit above what it refracts, and egui draws the whole
//! page into one surface, so the capsule is native: an `NSGlassEffectView`
//! laid over winit's view. Its controls push [`Command`]s that the next
//! frame turns into actions, and each frame hands it the playing state.

use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Mutex;

use egui::Rect;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSBezierPath, NSButton, NSClickGestureRecognizer, NSColor, NSControlSize,
    NSEvent, NSEventType, NSFont, NSFontWeightMedium, NSFontWeightRegular, NSGestureRecognizer,
    NSGlassEffectView, NSGlassEffectViewStyle, NSImage, NSImageSymbolConfiguration, NSImageView,
    NSLineBreakMode, NSSlider, NSTextAlignment, NSTextField, NSView,
};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSPoint, NSRect, NSSize, NSString};

use crate::app::App;
use crate::model::{Action, Page};
use crate::player::RepeatMode;

/// What the capsule's controls ask for.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Command {
    Button(Tag),
    /// The position, as a fraction, and whether the drag has ended.
    Seek(f64, bool),
    Volume(f64, bool),
    OpenSource,
    OpenArtist,
}

/// Each button's tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
enum Tag {
    Shuffle = 1,
    Previous,
    PlayPause,
    Next,
    Repeat,
    Heart,
    Lyrics,
    Queue,
    Devices,
    Mute,
    Seek,
    Volume,
}

impl Tag {
    const ALL: [Tag; 12] = [
        Tag::Shuffle,
        Tag::Previous,
        Tag::PlayPause,
        Tag::Next,
        Tag::Repeat,
        Tag::Heart,
        Tag::Lyrics,
        Tag::Queue,
        Tag::Devices,
        Tag::Mute,
        Tag::Seek,
        Tag::Volume,
    ];

    fn from_raw(raw: isize) -> Option<Tag> {
        Tag::ALL.into_iter().find(|tag| *tag as isize == raw)
    }
}

static COMMANDS: Mutex<Vec<Command>> = Mutex::new(Vec::new());
static CONTEXT: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

fn push(command: Command) {
    if let Ok(mut commands) = COMMANDS.lock() {
        commands.push(command);
    }
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SpotliteCapsuleTarget"]
    struct Target;

    impl Target {
        #[unsafe(method(onButton:))]
        fn on_button(&self, sender: &NSButton) {
            if let Some(tag) = Tag::from_raw(sender.tag()) {
                push(Command::Button(tag));
            }
        }

        #[unsafe(method(onSlider:))]
        fn on_slider(&self, sender: &NSSlider) {
            // A continuous slider calls on every move; the mouse coming
            // up is the end of the drag.
            let done = MainThreadMarker::new()
                .and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent())
                .is_none_or(|event| event.r#type() == NSEventType::LeftMouseUp);
            let value = sender.doubleValue();
            match Tag::from_raw(sender.tag()) {
                Some(Tag::Seek) => push(Command::Seek(value, done)),
                Some(Tag::Volume) => push(Command::Volume(value, done)),
                _ => {}
            }
        }

        #[unsafe(method(onTitle:))]
        fn on_title(&self, _sender: &NSGestureRecognizer) {
            push(Command::OpenSource);
        }

        #[unsafe(method(onSubtitle:))]
        fn on_subtitle(&self, _sender: &NSGestureRecognizer) {
            push(Command::OpenArtist);
        }
    }
);

define_class!(
    /// The glass's content, laid out from the top left.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SpotliteCapsuleContent"]
    struct Content;

    impl Content {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

/// The position shown, as `f64` bits, and whether the bar is held.
static SEEK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static SEEK_HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn seek_fraction() -> f64 {
    f64::from_bits(SEEK.load(std::sync::atomic::Ordering::Relaxed))
}

fn set_seek(bar: &SeekBar, fraction: f64) {
    if SEEK_HELD.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let fraction = fraction.clamp(0.0, 1.0);
    if (seek_fraction() - fraction).abs() > 1e-4 {
        SEEK.store(fraction.to_bits(), std::sync::atomic::Ordering::Relaxed);
        bar.setNeedsDisplay(true);
    }
}

define_class!(
    /// Music's position line: thin, knobless, and it seeks where pressed
    /// or dragged.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SpotliteSeekBar"]
    struct SeekBar;

    impl SeekBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let held = SEEK_HELD.load(std::sync::atomic::Ordering::Relaxed);
            let thickness = if held { 5.0 } else { 3.0 };
            let track = NSRect::new(
                NSPoint::new(0.0, (bounds.size.height - thickness) / 2.0),
                NSSize::new(bounds.size.width, thickness),
            );
            let radius = thickness / 2.0;
            NSColor::quaternaryLabelColor().setFill();
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(track, radius, radius).fill();
            let filled = NSRect::new(
                track.origin,
                NSSize::new(track.size.width * seek_fraction(), thickness),
            );
            if held {
                NSColor::labelColor().setFill();
            } else {
                NSColor::secondaryLabelColor().setFill();
            }
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(filled, radius, radius).fill();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            SEEK_HELD.store(true, std::sync::atomic::Ordering::Relaxed);
            self.seek_to(event, false);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.seek_to(event, false);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.seek_to(event, true);
            SEEK_HELD.store(false, std::sync::atomic::Ordering::Relaxed);
            self.setNeedsDisplay(true);
        }
    }
);

impl SeekBar {
    fn seek_to(&self, event: &NSEvent, done: bool) {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        let width = self.bounds().size.width.max(1.0);
        let fraction = (point.x / width).clamp(0.0, 1.0);
        SEEK.store(fraction.to_bits(), std::sync::atomic::Ordering::Relaxed);
        self.setNeedsDisplay(true);
        push(Command::Seek(fraction, done));
    }
}

/// What the capsule shows, compared frame to frame so AppKit only hears
/// about changes.
#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    size: [i32; 2],
    has_song: bool,
    title: String,
    subtitle: String,
    playing: bool,
    shuffle: bool,
    repeat: RepeatMode,
    saved: Option<bool>,
    lyrics: bool,
    queue: bool,
    remote: bool,
    muted: bool,
    art: Option<PathBuf>,
    position: String,
    remaining: String,
}

struct Capsule {
    glass: Retained<NSGlassEffectView>,
    target: Retained<Target>,
    buttons: Vec<(Tag, Retained<NSButton>)>,
    art: Retained<NSImageView>,
    title: Retained<NSTextField>,
    subtitle: Retained<NSTextField>,
    position: Retained<NSTextField>,
    remaining: Retained<NSTextField>,
    seek: Retained<SeekBar>,
    volume: Retained<NSSlider>,
    shown: Shown,
    /// The window's view the glass was laid on.
    host: Retained<NSView>,
}

thread_local! {
    static CAPSULE: RefCell<Option<Capsule>> = const { RefCell::new(None) };
}

/// The transport at the capsule's start and the gap between its buttons.
const TRANSPORT: [(Tag, f64); 5] = [
    (Tag::Shuffle, 28.0),
    (Tag::Previous, 32.0),
    (Tag::PlayPause, 38.0),
    (Tag::Next, 32.0),
    (Tag::Repeat, 28.0),
];
const GAP: f64 = 2.0;
/// The buttons before the volume slider at the capsule's end.
const EXTRAS: [Tag; 4] = [Tag::Lyrics, Tag::Queue, Tag::Devices, Tag::Mute];
const EXTRA_WIDTH: f64 = 30.0;
const VOLUME_WIDTH: f64 = 84.0;
const PADDING: f64 = 18.0;

fn symbol(name: &str, size: f64, medium: bool) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        None,
    )?;
    let weight = unsafe {
        if medium {
            NSFontWeightMedium
        } else {
            NSFontWeightRegular
        }
    };
    let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(size, weight);
    image.imageWithSymbolConfiguration(&config)
}

fn label(mtm: MainThreadMarker, size: f64, medium: bool) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(""), mtm);
    let weight = unsafe {
        if medium {
            NSFontWeightMedium
        } else {
            NSFontWeightRegular
        }
    };
    field.setFont(Some(&NSFont::systemFontOfSize_weight(size, weight)));
    field.setAlignment(NSTextAlignment::Center);
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    field.setSelectable(false);
    field
}

fn set_tooltip(view: &NSView, text: &str) {
    view.setToolTip(Some(&NSString::from_str(text)));
}

fn tint(button: &NSButton, color: &NSColor) {
    button.setContentTintColor(Some(color));
}

impl Capsule {
    fn new(mtm: MainThreadMarker, host: Retained<NSView>) -> Capsule {
        let target: Retained<Target> = unsafe { msg_send![Target::alloc(mtm), init] };
        let glass = NSGlassEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        glass.setStyle(NSGlassEffectViewStyle::Regular);
        let content: Retained<Content> = unsafe { msg_send![Content::alloc(mtm), init] };
        glass.setContentView(Some(&content));

        let mut buttons = Vec::new();
        for tag in TRANSPORT
            .iter()
            .map(|(tag, _)| *tag)
            .chain([Tag::Heart])
            .chain(EXTRAS)
        {
            let button = NSButton::initWithFrame(mtm.alloc(), NSRect::ZERO);
            button.setBordered(false);
            button.setTitle(&NSString::from_str(""));
            button.setTag(tag as isize);
            button.setRefusesFirstResponder(true);
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(onButton:)));
            }
            content.addSubview(&button);
            buttons.push((tag, button));
        }

        let art = NSImageView::initWithFrame(mtm.alloc(), NSRect::ZERO);
        art.setWantsLayer(true);
        round(&art, 5.0);
        content.addSubview(&art);

        let title = label(mtm, 12.5, true);
        let subtitle = label(mtm, 11.5, false);
        subtitle.setTextColor(Some(&NSColor::secondaryLabelColor()));
        for (field, action) in [(&title, sel!(onTitle:)), (&subtitle, sel!(onSubtitle:))] {
            let click = unsafe {
                NSClickGestureRecognizer::initWithTarget_action(
                    mtm.alloc(),
                    Some(&target),
                    Some(action),
                )
            };
            field.addGestureRecognizer(&click);
            content.addSubview(field);
        }
        let position = label(mtm, 10.0, false);
        let remaining = label(mtm, 10.0, false);
        for field in [&position, &remaining] {
            field.setTextColor(Some(&NSColor::secondaryLabelColor()));
            field.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
                10.0,
                unsafe { NSFontWeightRegular },
            )));
            content.addSubview(field);
        }
        position.setAlignment(NSTextAlignment::Right);
        remaining.setAlignment(NSTextAlignment::Left);

        let slider = |tag: Tag| {
            let slider = NSSlider::initWithFrame(mtm.alloc(), NSRect::ZERO);
            slider.setMinValue(0.0);
            slider.setMaxValue(1.0);
            slider.setContinuous(true);
            slider.setControlSize(NSControlSize::Mini);
            slider.setRefusesFirstResponder(true);
            slider.setTag(tag as isize);
            unsafe {
                slider.setTarget(Some(&target));
                slider.setAction(Some(sel!(onSlider:)));
            }
            content.addSubview(&slider);
            slider
        };
        let seek: Retained<SeekBar> = unsafe { msg_send![SeekBar::alloc(mtm), init] };
        content.addSubview(&seek);
        let volume = slider(Tag::Volume);
        set_tooltip(&volume, "Volume");

        host.addSubview(&glass);
        Capsule {
            glass,
            target,
            buttons,
            art,
            title,
            subtitle,
            position,
            remaining,
            seek,
            volume,
            shown: Shown::default(),
            host,
        }
    }

    fn button(&self, tag: Tag) -> &NSButton {
        &self
            .buttons
            .iter()
            .find(|(each, _)| *each == tag)
            .expect("every tag has a button")
            .1
    }

    /// Lays the controls out for a capsule `size` points wide and tall,
    /// and answers where the devices button is, from the capsule's top left.
    fn layout(&self, width: f64, height: f64) -> NSRect {
        let middle = height / 2.0;
        let mut x = PADDING;
        for (tag, w) in TRANSPORT {
            self.button(tag).setFrame(NSRect::new(
                NSPoint::new(x, middle - 18.0),
                NSSize::new(w, 36.0),
            ));
            x += w + GAP;
        }
        let transport_end = x - GAP;

        let mut right = width - PADDING;
        self.volume.setFrame(NSRect::new(
            NSPoint::new(right - VOLUME_WIDTH, middle - 8.0),
            NSSize::new(VOLUME_WIDTH, 16.0),
        ));
        right -= VOLUME_WIDTH + 4.0;
        let mut devices = NSRect::ZERO;
        for tag in EXTRAS.iter().rev() {
            let frame = NSRect::new(
                NSPoint::new(right - EXTRA_WIDTH, middle - 15.0),
                NSSize::new(EXTRA_WIDTH, 30.0),
            );
            self.button(*tag).setFrame(frame);
            if *tag == Tag::Devices {
                devices = frame;
            }
            right -= EXTRA_WIDTH + 2.0;
        }

        // The song's display, between the transport and the end.
        let left = transport_end + 16.0;
        let right = right - 14.0;
        let roomy = right - left > 160.0;
        for view in [
            &*self.art as &NSView,
            &self.title,
            &self.subtitle,
            &self.position,
            &self.remaining,
            &self.seek,
            self.button(Tag::Heart),
        ] {
            view.setHidden(!roomy);
        }
        if !roomy {
            return devices;
        }
        let art = height - 14.0;
        self.art.setFrame(NSRect::new(
            NSPoint::new(left, (height - art) / 2.0),
            NSSize::new(art, art),
        ));
        let words_left = left + art + 10.0;
        let words_right = right - 28.0;
        let words = words_right - words_left;
        self.title.setFrame(NSRect::new(
            NSPoint::new(words_left, 6.0),
            NSSize::new(words, 17.0),
        ));
        self.subtitle.setFrame(NSRect::new(
            NSPoint::new(words_left + 30.0, 21.0),
            NSSize::new((words - 60.0).max(20.0), 15.0),
        ));
        let time = 34.0;
        self.position.setFrame(NSRect::new(
            NSPoint::new(words_left, 37.0),
            NSSize::new(time, 13.0),
        ));
        self.remaining.setFrame(NSRect::new(
            NSPoint::new(words_right - time, 37.0),
            NSSize::new(time, 13.0),
        ));
        self.seek.setFrame(NSRect::new(
            NSPoint::new(words_left + time + 6.0, 37.0),
            NSSize::new((words - 2.0 * (time + 6.0)).max(20.0), 13.0),
        ));
        self.button(Tag::Heart).setFrame(NSRect::new(
            NSPoint::new(right - 26.0, middle - 13.0),
            NSSize::new(26.0, 26.0),
        ));
        devices
    }

    fn update(&mut self, shown: Shown) {
        let old = std::mem::replace(&mut self.shown, shown.clone());
        let label = NSColor::labelColor();
        let secondary = NSColor::secondaryLabelColor();
        let accent = NSColor::controlAccentColor();
        let set = |tag: Tag, name: &str, size: f64, color: &NSColor, tip: &str| {
            let button = self.button(tag);
            button.setImage(symbol(name, size, false).as_deref());
            tint(button, color);
            set_tooltip(button, tip);
        };
        let first = old == Shown::default();
        if first || old.shuffle != shown.shuffle {
            set(
                Tag::Shuffle,
                "shuffle",
                13.0,
                if shown.shuffle { &accent } else { &secondary },
                "Shuffle",
            );
        }
        if first {
            set(Tag::Previous, "backward.fill", 17.0, &label, "Previous");
            set(Tag::Next, "forward.fill", 17.0, &label, "Next");
            set(Tag::Lyrics, "quote.bubble", 15.0, &secondary, "Lyrics");
            set(Tag::Queue, "list.bullet", 15.0, &secondary, "Queue");
        }
        if first || old.playing != shown.playing {
            let (name, tip) = if shown.playing {
                ("pause.fill", "Pause")
            } else {
                ("play.fill", "Play")
            };
            set(Tag::PlayPause, name, 22.0, &label, tip);
        }
        if first || old.repeat != shown.repeat {
            let (name, color, tip) = match shown.repeat {
                RepeatMode::Off => ("repeat", &secondary, "Repeat"),
                RepeatMode::Context => ("repeat", &accent, "Repeat one"),
                RepeatMode::Track => ("repeat.1", &accent, "Repeat off"),
            };
            set(Tag::Repeat, name, 13.0, color, tip);
        }
        if first || old.lyrics != shown.lyrics {
            tint(
                self.button(Tag::Lyrics),
                if shown.lyrics { &accent } else { &secondary },
            );
        }
        if first || old.queue != shown.queue {
            tint(
                self.button(Tag::Queue),
                if shown.queue { &accent } else { &secondary },
            );
        }
        if first || old.remote != shown.remote {
            set(
                Tag::Devices,
                "hifispeaker",
                15.0,
                if shown.remote { &accent } else { &secondary },
                "Connect to a device",
            );
        }
        if first || old.muted != shown.muted {
            let (name, tip) = if shown.muted {
                ("speaker.slash.fill", "Unmute")
            } else {
                ("speaker.wave.2.fill", "Mute")
            };
            set(Tag::Mute, name, 14.0, &secondary, tip);
        }
        if first || old.saved != shown.saved {
            let heart = self.button(Tag::Heart);
            match shown.saved {
                Some(saved) => {
                    heart.setHidden(false);
                    let (name, color, tip) = if saved {
                        ("heart.fill", &accent, "Remove from Liked Songs")
                    } else {
                        ("heart", &secondary, "Save to Liked Songs")
                    };
                    set(Tag::Heart, name, 14.0, color, tip);
                }
                None => heart.setHidden(true),
            }
        }
        if first || old.title != shown.title || old.has_song != shown.has_song {
            let text = if shown.has_song {
                shown.title.as_str()
            } else {
                "Nothing playing"
            };
            self.title.setStringValue(&NSString::from_str(text));
            self.title
                .setTextColor(Some(if shown.has_song { &label } else { &secondary }));
        }
        if first || old.subtitle != shown.subtitle {
            self.subtitle
                .setStringValue(&NSString::from_str(&shown.subtitle));
        }
        if old.position != shown.position {
            self.position
                .setStringValue(&NSString::from_str(&shown.position));
        }
        if old.remaining != shown.remaining {
            self.remaining
                .setStringValue(&NSString::from_str(&shown.remaining));
        }
        if first || old.art != shown.art {
            let image = shown
                .art
                .as_ref()
                .and_then(|path| std::fs::read(path).ok())
                .and_then(|bytes| {
                    let mtm = MainThreadMarker::new()?;
                    NSImage::initWithData(mtm.alloc(), &NSData::with_bytes(&bytes))
                })
                .or_else(|| symbol("music.note", 16.0, false));
            self.art.setImage(image.as_deref());
            self.art
                .setContentTintColor(Some(&NSColor::tertiaryLabelColor()));
        }
    }
}

fn round(view: &NSView, radius: f64) {
    // SAFETY: a layer-backed view's layer is a CALayer, which takes these.
    unsafe {
        let layer: Option<Retained<AnyObject>> = msg_send![view, layer];
        if let Some(layer) = layer {
            let () = msg_send![&*layer, setCornerRadius: radius];
            let () = msg_send![&*layer, setMasksToBounds: true];
        }
    }
}

/// Turns what the controls asked for since the last frame into actions.
fn apply_commands(app: &mut App) {
    let commands = COMMANDS
        .lock()
        .map(|mut commands| std::mem::take(&mut *commands))
        .unwrap_or_default();
    let now = app.now_playing();
    for command in commands {
        match command {
            Command::Button(tag) => {
                let action = match tag {
                    Tag::Shuffle => Action::ToggleShuffle,
                    Tag::Previous => Action::Previous,
                    Tag::PlayPause => Action::TogglePlay,
                    Tag::Next => Action::Next,
                    Tag::Repeat => Action::CycleRepeat,
                    Tag::Heart => match &now {
                        Some(now) => Action::ToggleSaved(now.uri.clone()),
                        None => continue,
                    },
                    Tag::Lyrics => Action::ToggleLyricsPanel,
                    Tag::Queue => Action::ToggleQueuePanel,
                    Tag::Devices => Action::ToggleDevicesPopup,
                    Tag::Mute => Action::ToggleMute,
                    Tag::Seek | Tag::Volume => continue,
                };
                app.actions.push(action);
            }
            Command::Seek(value, done) => {
                let value = value as f32;
                if done {
                    app.seek_preview = None;
                    if let Some(now) = &now
                        && now.duration_ms > 0
                    {
                        app.actions
                            .push(Action::Seek((value * now.duration_ms as f32) as u32));
                    }
                } else {
                    app.seek_preview = Some(value);
                }
            }
            Command::Volume(value, done) => {
                let percent = (value * 100.0).round() as u8;
                if done {
                    app.volume_preview = None;
                    app.actions.push(Action::SetVolume(percent));
                } else {
                    app.volume_preview = Some(value as f32);
                    // Local volume is cheap to apply continuously; remote
                    // goes on release.
                    if now.as_ref().is_none_or(|now| now.local) {
                        app.actions.push(Action::PreviewVolume(percent));
                    }
                }
            }
            Command::OpenSource => {
                if let Some(now) = &now {
                    if let Some(id) = &now.album_id {
                        app.actions.push(Action::Open(Page::Album(id.clone())));
                    } else if let Some(id) = &now.show_id {
                        app.actions.push(Action::Open(Page::Show(id.clone())));
                    }
                }
            }
            Command::OpenArtist => {
                if let Some(now) = &now {
                    if let Some(id) = now.artists.iter().find_map(|artist| artist.id.clone()) {
                        app.actions.push(Action::Open(Page::Artist(id)));
                    } else if let Some(id) = &now.show_id {
                        app.actions.push(Action::Open(Page::Show(id.clone())));
                    }
                }
            }
        }
    }
}

/// Shows the capsule over `capsule`, in egui points, with the playing
/// state, after acting on what its controls asked for.
pub fn show(app: &mut App, ctx: &egui::Context, capsule: Rect) {
    let _ = CONTEXT.set(ctx.clone());
    apply_commands(app);
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(host) = crate::mac_glass::content_view() else {
        return;
    };
    let now = app.now_playing();
    let volume = now
        .as_ref()
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let (position, duration) = now
        .as_ref()
        .map(|now| (now.position_ms, now.duration_ms))
        .unwrap_or((0, 0));
    let shown_position = match app.seek_preview {
        Some(fraction) => (fraction * duration as f32) as u32,
        None => position,
    };
    let art = now.as_ref().and_then(|now| {
        let url = now.art_small.as_deref().or(now.art_url.as_deref())?;
        app.media_art_file(ctx, url)
    });
    let zoom = f64::from(ctx.zoom_factor());
    let shown = Shown {
        size: [
            (capsule.width() * 2.0).round() as i32,
            (capsule.height() * 2.0).round() as i32,
        ],
        has_song: now.is_some(),
        title: now
            .as_ref()
            .map(|now| now.title.clone())
            .unwrap_or_default(),
        subtitle: now
            .as_ref()
            .map(|now| now.subtitle.clone())
            .unwrap_or_default(),
        playing: now.as_ref().is_some_and(|now| now.playing),
        shuffle: now
            .as_ref()
            .map_or_else(|| app.playing_context_shuffle(), |now| now.shuffle),
        repeat: now.as_ref().map(|now| now.repeat).unwrap_or_default(),
        saved: now
            .as_ref()
            .filter(|now| !now.is_episode)
            .map(|now| app.is_saved(&now.uri).unwrap_or(false)),
        lyrics: app.show_lyrics_panel,
        queue: app.show_queue_panel || matches!(app.page(), Page::Queue),
        remote: now.as_ref().is_some_and(|now| !now.local),
        muted: volume == 0,
        art,
        position: if now.is_some() {
            crate::util::format_duration_ms(shown_position)
        } else {
            String::new()
        },
        remaining: if now.is_some() {
            format!(
                "-{}",
                crate::util::format_duration_ms(duration.saturating_sub(shown_position))
            )
        } else {
            String::new()
        },
    };
    let fraction = if duration > 0 {
        f64::from(shown_position) / f64::from(duration)
    } else {
        0.0
    };
    let devices = CAPSULE.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.as_ref().is_some_and(|capsule| capsule.host != host)
            && let Some(old) = cell.take()
        {
            old.glass.removeFromSuperview();
        }
        let capsule_view = cell.get_or_insert_with(|| Capsule::new(mtm, host.clone()));
        let height = host.frame().size.height;
        let rect = capsule * zoom as f32;
        let y = if host.isFlipped() {
            f64::from(rect.top())
        } else {
            height - f64::from(rect.bottom())
        };
        let frame = NSRect::new(
            NSPoint::new(f64::from(rect.left()), y),
            NSSize::new(f64::from(rect.width()), f64::from(rect.height())),
        );
        let resized = capsule_view.glass.frame() != frame;
        if resized {
            capsule_view.glass.setFrame(frame);
            capsule_view
                .glass
                .setCornerRadius(f64::from(rect.height()) / 2.0);
        }
        capsule_view.glass.setHidden(false);
        let devices = capsule_view.layout(frame.size.width, frame.size.height);
        capsule_view.update(shown);
        // A slider being dragged keeps the value under the pointer.
        if app.seek_preview.is_none() {
            set_seek(&capsule_view.seek, fraction);
        }
        if app.volume_preview.is_none() {
            capsule_view
                .volume
                .setDoubleValue(f64::from(volume) / 100.0);
        }
        capsule_view
            .volume
            .setEnabled(now.as_ref().is_none_or(|now| now.can_set_volume));
        let _ = &capsule_view.target;
        devices
    });
    // The devices popup opens above its button.
    let devices = Rect::from_min_size(
        capsule.min + egui::vec2(devices.origin.x as f32, devices.origin.y as f32) / zoom as f32,
        egui::vec2(devices.size.width as f32, devices.size.height as f32) / zoom as f32,
    );
    ctx.data_mut(|data| {
        data.insert_temp(egui::Id::new(crate::ui::devices::BUTTON_RECT_ID), devices)
    });
    // The glass and AppKit's controls animate on their own; egui only has
    // to repaint for the position.
    if now.as_ref().is_some_and(|now| now.playing) {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
}

/// Takes the capsule away, as on the sign-in screen.
pub fn hide() {
    CAPSULE.with(|cell| {
        if let Some(capsule) = cell.borrow().as_ref() {
            capsule.glass.setHidden(true);
        }
    });
}
