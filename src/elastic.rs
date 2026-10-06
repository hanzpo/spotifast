//! The rubber band at a scroll area's edge, as AppKit's scroll views have it.
//!
//! Fingers that drag the content past the top or bottom keep moving it,
//! ever more stiffly, and when they lift it springs back. A glide that runs
//! into the edge bounces once, as far as its speed carries it, and the rest
//! of the glide is spent: it does not keep pressing against the edge.
//!
//! egui leaves the scroll delta unused when an area cannot scroll further,
//! so whatever is left after the area has drawn is the overscroll. The
//! content's shapes are then moved by the stretch inside the area's own
//! clip, so nothing spills past its edges.
//!
//! Only touchpad input stretches: a mouse wheel stops at the edge.

use egui::layers::ShapeIdx;
use egui::{Context, Event, Id, MouseWheelUnit, Rect, TouchPhase, Ui, Vec2};

/// How stiff the band is under the fingers: Apple's constant for
/// `UIScrollView`.
const STIFFNESS: f32 = 0.55;
/// The share of an area's length the band can stretch to.
const REACH: f32 = 1.0 / 3.0;
/// The return spring's natural frequency, per second. Critically damped, it
/// settles in about half a second and never overshoots the edge.
const SPRING: f32 = 12.0;
/// The fastest a glide may hit the edge, in points per second, so a hard
/// fling bounces no further than about a hundred points.
const MAX_BOUNCE_SPEED: f32 = 3200.0;
/// Below this many points, and this speed, the band is at rest.
const AT_REST: f32 = 0.3;
const AT_REST_SPEED: f32 = 6.0;

/// Where a touchpad gesture is: fingers on the pad, the glide after they
/// lift, or neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Gesture {
    #[default]
    Idle,
    Touching,
    /// Fingers lifted; a glide may follow.
    Lifted,
    Gliding,
}

#[derive(Clone, Copy, Debug, Default)]
struct Input {
    pass: u64,
    gesture: Gesture,
    /// Whether this pass's scrolling came from a touchpad, in points.
    precise: bool,
}

/// One area's band.
#[derive(Clone, Copy, Debug)]
struct Band {
    /// How far the content stands past its edge, in points.
    stretch: f32,
    /// How fast it is moving, in points per second, while it springs.
    speed: f32,
    /// This glide has already bounced, so the rest of it is spent.
    spent: bool,
    rect: Rect,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            stretch: 0.0,
            speed: 0.0,
            spent: false,
            rect: Rect::NOTHING,
        }
    }
}

impl Band {
    fn at_rest(&self) -> bool {
        self.stretch == 0.0 && self.speed == 0.0
    }

    /// The fingers move the band by `delta` more, against its resistance.
    fn pull(&mut self, delta: f32, extent: f32) {
        let pulled = unrubber(self.stretch, extent) + delta;
        // Pulled back past the edge, the band is simply gone.
        self.stretch = if self.stretch != 0.0 && pulled.signum() != self.stretch.signum() {
            0.0
        } else {
            rubber(pulled, extent)
        };
        self.speed = 0.0;
    }

    /// A glide meets the edge at `speed`: one bounce, then nothing more.
    fn bounce(&mut self, speed: f32) {
        if !self.spent {
            self.speed = speed.clamp(-MAX_BOUNCE_SPEED, MAX_BOUNCE_SPEED);
            self.spent = true;
        }
    }

    /// Lets the spring run for `dt` seconds: the exact critically damped
    /// motion toward the edge, so a slow frame cannot make it overshoot.
    fn settle(&mut self, dt: f32) {
        let (x, v) = (self.stretch, self.speed);
        let decay = (-SPRING * dt).exp();
        let lead = v + SPRING * x;
        self.stretch = (x + lead * dt) * decay;
        self.speed = (v - SPRING * lead * dt) * decay;
        if self.stretch.abs() < AT_REST && self.speed.abs() < AT_REST_SPEED {
            self.stretch = 0.0;
            self.speed = 0.0;
        }
    }
}

/// The farthest the band ever stretches: a third of the area, as stiff as
/// AppKit's own scroll views feel.
fn extent(rect: Rect, axis: usize) -> f32 {
    (rect.size()[axis] * REACH).max(1.0)
}

/// How far content moves when pulled `pulled` points past the edge of an
/// area `extent` long: nearly one to one at first, never as far as `extent`.
pub fn rubber(pulled: f32, extent: f32) -> f32 {
    let distance = pulled.abs();
    let moved = (1.0 - 1.0 / (distance * STIFFNESS / extent + 1.0)) * extent;
    moved.copysign(pulled)
}

/// The pull that moves content `stretch` points: [`rubber`] undone.
fn unrubber(stretch: f32, extent: f32) -> f32 {
    let fraction = (stretch.abs() / extent).min(0.999);
    (extent / STIFFNESS * (1.0 / (1.0 - fraction) - 1.0)).copysign(stretch)
}

const INPUT: &str = "elastic-input";

/// The gesture as of this pass, read from the pass's scroll events once.
fn input(ctx: &Context) -> Input {
    let pass = ctx.cumulative_pass_nr();
    let id = Id::new(INPUT);
    let mut state = ctx.data(|data| data.get_temp::<Input>(id).unwrap_or_default());
    if state.pass == pass && pass != 0 {
        return state;
    }
    state.pass = pass;
    state.precise = false;
    ctx.input(|input| {
        for event in &input.events {
            if let Event::MouseWheel { unit, phase, .. } = event {
                state.precise |= *unit == MouseWheelUnit::Point;
                state.gesture = match (state.gesture, phase) {
                    (Gesture::Lifted, TouchPhase::Start) => Gesture::Gliding,
                    (_, TouchPhase::Start) => Gesture::Touching,
                    (Gesture::Touching, TouchPhase::End | TouchPhase::Cancel) => Gesture::Lifted,
                    (_, TouchPhase::End | TouchPhase::Cancel) => Gesture::Idle,
                    (gesture, TouchPhase::Move) => gesture,
                };
            }
        }
    });
    ctx.data_mut(|data| data.insert_temp(id, state));
    state
}

/// Shows `area` with a rubber band along `axes` when exactly one axis
/// scrolls.
pub fn show<R>(
    ui: &mut Ui,
    area: egui::ScrollArea,
    axes: egui::Vec2b,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::scroll_area::ScrollAreaOutput<R> {
    let axis = match (axes.x, axes.y) {
        (true, false) => 0,
        (false, true) => 1,
        _ => return area.show(ui, contents),
    };
    let ctx = ui.ctx().clone();
    let gesture = input(&ctx);
    let layer = ui.layer_id();
    let corner = ui.cursor().min.round();
    let key = Id::new(("elastic-place", layer, corner.x as i32, corner.y as i32));
    let known: Option<Id> = ctx.data(|data| data.get_temp(key));
    let pointer = ctx.pointer_latest_pos();
    let dt = ctx.input(|input| input.stable_dt).clamp(1.0 / 240.0, 0.05);

    // While the content stands past its edge, the gesture moves the band,
    // not the content: fingers pull it further or let it back, and a glide
    // that already bounced is spent.
    if let Some(id) = known
        && let Some(mut band) = ctx.data(|data| data.get_temp::<Band>(id))
        && !band.at_rest()
        && gesture.precise
        && pointer.is_some_and(|pos| band.rect.contains(pos))
    {
        let delta = ctx.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta[axis]));
        if delta != 0.0 && gesture.gesture == Gesture::Touching {
            band.pull(delta, extent(band.rect, axis));
            ctx.data_mut(|data| data.insert_temp(id, band));
        }
    }

    let start = ctx.graphics(|graphics| {
        graphics
            .get(layer)
            .map_or(ShapeIdx(0), |list| list.next_idx())
    });
    let output = area.show(ui, contents);
    let id = output.id;
    ctx.data_mut(|data| data.insert_temp(key, id));

    let mut band = ctx
        .data(|data| data.get_temp::<Band>(id))
        .unwrap_or_default();
    band.rect = output.inner_rect;
    if gesture.gesture != Gesture::Gliding {
        band.spent = false;
    }
    let hovered = pointer.is_some_and(|pos| output.inner_rect.contains(pos));
    let scrollable = output.content_size[axis] > output.inner_rect.size()[axis] + 0.5;

    // What the area left unused is pressing against its edge.
    if hovered && scrollable && gesture.precise && gesture.gesture != Gesture::Idle {
        let left = ctx.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta[axis]));
        let max = output.content_size[axis] - output.inner_rect.size()[axis];
        let offset = output.state.offset[axis];
        let at_start = offset <= 0.5 && left > 0.0;
        let at_end = offset >= max - 0.5 && left < 0.0;
        if at_start || at_end {
            match gesture.gesture {
                Gesture::Touching => band.pull(left, extent(band.rect, axis)),
                _ => band.bounce(left / dt),
            }
        }
    }

    // Unless fingers hold it, the band springs back.
    if !band.at_rest() && gesture.gesture != Gesture::Touching {
        band.settle(dt);
    }
    if !band.at_rest() {
        ctx.request_repaint();
    }

    if band.stretch != 0.0 {
        let mut shift = Vec2::ZERO;
        shift[axis] = band.stretch;
        ctx.graphics_mut(|graphics| {
            let list = graphics.entry(layer);
            for index in start.0..list.next_idx().0 {
                list.mutate_shape(ShapeIdx(index), |clipped| clipped.shape.translate(shift));
            }
        });
    }
    ctx.data_mut(|data| data.insert_temp(id, band));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_gives_way_one_to_one_then_stiffens() {
        let extent = 600.0;
        let small = rubber(4.0, extent);
        assert!((small - 4.0 * STIFFNESS).abs() < 0.2, "{small}");
        let far = rubber(10_000.0, extent);
        assert!(far < extent && far > extent * 0.9, "{far}");
        assert_eq!(rubber(-50.0, extent), -rubber(50.0, extent));
    }

    #[test]
    fn undoing_the_band_finds_the_pull() {
        for pulled in [-900.0, -12.0, 3.0, 250.0] {
            let back = unrubber(rubber(pulled, 500.0), 500.0);
            assert!((back - pulled).abs() < 0.01, "{pulled} -> {back}");
        }
    }

    #[test]
    fn a_released_band_returns_without_crossing_the_edge() {
        let mut band = Band {
            stretch: 120.0,
            ..Band::default()
        };
        let mut steps = 0;
        while !band.at_rest() {
            band.settle(1.0 / 60.0);
            assert!(band.stretch >= 0.0, "overshot to {}", band.stretch);
            steps += 1;
        }
        assert!((20..60).contains(&steps), "settled in {steps} frames");
    }

    fn wheel(phase: TouchPhase, delta: f32) -> Event {
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: Vec2::new(0.0, delta),
            phase,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// Runs one pass with `events` and reports the stretch of the area.
    fn pass(ctx: &Context, events: Vec<Event>, time: f64) -> f32 {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(400.0, 300.0),
            )),
            events: [vec![Event::PointerMoved(egui::pos2(200.0, 150.0))], events].concat(),
            time: Some(time),
            ..Default::default()
        };
        let mut stretch = 0.0;
        let mut output = ctx.run_ui(input, |ui| {
            let output = show(
                ui,
                egui::ScrollArea::vertical().animated(false),
                egui::Vec2b::new(false, true),
                |ui| {
                    for row in 0..100 {
                        ui.label(format!("Row {row}"));
                    }
                },
            );
            stretch = ui
                .ctx()
                .data(|data| data.get_temp::<Band>(output.id))
                .map_or(0.0, |band| band.stretch);
        });
        output.textures_delta.clear();
        stretch
    }

    struct Clock {
        ctx: Context,
        time: f64,
    }

    impl Clock {
        fn new() -> Self {
            let mut clock = Self {
                ctx: Context::default(),
                time: 0.0,
            };
            clock.step(vec![]);
            clock
        }

        fn step(&mut self, events: Vec<Event>) -> f32 {
            self.time += 1.0 / 60.0;
            pass(&self.ctx, events, self.time)
        }
    }

    #[test]
    fn fingers_pull_past_the_top_and_the_band_springs_back_when_they_lift() {
        let mut clock = Clock::new();
        clock.step(vec![wheel(TouchPhase::Start, 0.0)]);
        let mut stretch = 0.0;
        for _ in 0..10 {
            stretch = clock.step(vec![wheel(TouchPhase::Move, 20.0)]);
        }
        assert!(stretch > 20.0 && stretch < 200.0, "{stretch}");

        // #then holding still keeps it, and lifting returns it to rest
        assert_eq!(clock.step(vec![]), stretch);
        clock.step(vec![wheel(TouchPhase::End, 0.0)]);
        for _ in 0..60 {
            stretch = clock.step(vec![]);
        }
        assert_eq!(stretch, 0.0);
    }

    /// Scrolling on at the top keeps the band in reach: it stiffens rather
    /// than following the fingers away.
    #[test]
    fn scrolling_on_at_the_top_stiffens_rather_than_running_away() {
        let mut clock = Clock::new();
        clock.step(vec![wheel(TouchPhase::Start, 0.0)]);
        let mut stretch = 0.0;
        for _ in 0..120 {
            stretch = clock.step(vec![wheel(TouchPhase::Move, 25.0)]);
        }
        assert!(stretch < 300.0 * REACH, "{stretch}");
    }

    /// A glide that hits the top bounces once and comes back while the
    /// rest of the glide is still arriving.
    #[test]
    fn a_glide_into_the_top_bounces_once_and_returns() {
        let mut clock = Clock::new();
        clock.step(vec![wheel(TouchPhase::Start, 0.0)]);
        clock.step(vec![wheel(TouchPhase::Move, 30.0)]);
        clock.step(vec![wheel(TouchPhase::End, 0.0)]);
        clock.step(vec![wheel(TouchPhase::Start, 0.0)]);
        let mut peak: f32 = 0.0;
        let mut stretch = 0.0;
        // A long glide whose deltas fade out over two seconds.
        for frame in 0..120 {
            let delta = 40.0 * (1.0 - frame as f32 / 120.0);
            stretch = clock.step(vec![wheel(TouchPhase::Move, delta)]);
            peak = peak.max(stretch);
        }
        assert!(peak > 10.0 && peak < 120.0, "peaked at {peak}");
        // #then it is back at the edge before the glide even ends
        assert_eq!(stretch, 0.0);
        clock.step(vec![wheel(TouchPhase::End, 0.0)]);
    }

    #[test]
    fn a_mouse_wheel_stops_at_the_edge() {
        let ctx = Context::default();
        pass(&ctx, vec![], 0.0);
        let stretch = pass(
            &ctx,
            vec![Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                delta: Vec2::new(0.0, 3.0),
                phase: TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            }],
            0.1,
        );
        assert_eq!(stretch, 0.0);
    }
}
