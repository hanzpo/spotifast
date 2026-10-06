//! The rubber band at a scroll area's edge, as AppKit's scroll views have it.
//!
//! A touchpad gesture that reaches the top or bottom keeps moving the
//! content, ever more stiffly, and when the fingers lift (or the glide that
//! follows runs into the edge) it springs back. egui leaves the scroll delta
//! unused when an area cannot scroll further, so whatever is left after the
//! area has drawn is the overscroll. The content's shapes are then moved by
//! the stretch inside the area's own clip, so nothing spills past its edges.
//!
//! Only touchpad input stretches: a mouse wheel stops at the edge.

use egui::layers::ShapeIdx;
use egui::{Context, Event, Id, LayerId, MouseWheelUnit, Rect, TouchPhase, Ui, Vec2};

/// How stiff the band is: Apple's constant for `UIScrollView`.
const STIFFNESS: f32 = 0.55;
/// How fast a released band returns, per second. About a third of a second
/// to settle.
const RETURN_RATE: f32 = 12.0;
/// Below this many points the band is at rest.
const AT_REST: f32 = 0.25;

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

/// One area's band: the overscroll it has taken, and where it sits.
#[derive(Clone, Copy, Debug)]
struct Band {
    /// The overscroll the gesture asked for, before the band's resistance.
    pulled: f32,
    rect: Rect,
    layer: Option<LayerId>,
    axis: usize,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            pulled: 0.0,
            rect: Rect::NOTHING,
            layer: None,
            axis: 1,
        }
    }
}

impl Band {
    fn stretch(&self) -> f32 {
        rubber(self.pulled, extent(self.rect, self.axis))
    }
}

fn extent(rect: Rect, axis: usize) -> f32 {
    rect.size()[axis].max(1.0)
}

/// How far content moves when pulled `pulled` points past the edge of an
/// area `extent` long: one to one at first, never as far as `extent`.
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

    // A stretched band takes the gesture first, so moving back toward the
    // content relaxes the band before the content scrolls.
    if let Some(id) = known
        && let Some(mut band) = ctx.data(|data| data.get_temp::<Band>(id))
        && band.pulled != 0.0
        && pointer.is_some_and(|pos| band.rect.contains(pos))
    {
        let delta = ctx.input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta[axis]));
        if delta != 0.0 {
            let pulled = band.pulled + delta;
            // Past the rest point the band is gone; the rest of this
            // movement is dropped rather than scrolled, for one pass.
            band.pulled = if pulled.signum() == band.pulled.signum() {
                pulled
            } else {
                0.0
            };
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
    band.layer = Some(layer);
    band.axis = axis;
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
            band.pulled += left;
        }
    }

    // Released, or carried by a glide: spring back.
    if band.pulled != 0.0 && gesture.gesture != Gesture::Touching {
        let extent = extent(band.rect, axis);
        let dt = ctx.input(|input| input.stable_dt).min(0.1);
        let stretch = band.stretch() * (-RETURN_RATE * dt).exp();
        band.pulled = if stretch.abs() < AT_REST {
            0.0
        } else {
            unrubber(stretch, extent)
        };
    }
    if band.pulled != 0.0 {
        ctx.request_repaint();
    }

    let stretch = band.stretch();
    if stretch != 0.0 {
        let mut shift = Vec2::ZERO;
        shift[axis] = stretch;
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
                .map_or(0.0, |band| band.stretch());
        });
        output.textures_delta.clear();
        stretch
    }

    #[test]
    fn a_pull_past_the_top_stretches_and_springs_back_on_release() {
        let ctx = Context::default();
        let mut time = 0.0;
        let mut step = |events| {
            time += 1.0 / 60.0;
            pass(&ctx, events, time)
        };
        step(vec![]);
        step(vec![wheel(TouchPhase::Start, 0.0)]);
        let mut stretch = 0.0;
        for _ in 0..10 {
            stretch = step(vec![wheel(TouchPhase::Move, 20.0)]);
        }
        assert!(stretch > 20.0 && stretch < 200.0, "{stretch}");

        // #then holding still keeps it, and lifting returns it to rest
        assert_eq!(step(vec![]), stretch);
        step(vec![wheel(TouchPhase::End, 0.0)]);
        for _ in 0..60 {
            stretch = step(vec![]);
        }
        assert_eq!(stretch, 0.0);
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
