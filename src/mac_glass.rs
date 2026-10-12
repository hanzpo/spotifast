//! The sidebar's glass, and the system accent colour.
//!
//! The window is transparent. Beneath winit's content view, in the window's
//! frame view, sit two AppKit views: a plain one in the window colour that
//! fills the sidebar's column, and on it an `NSGlassEffectView`, Liquid
//! Glass inset and rounded the way macOS's own sidebars float.
//! egui paints everything but the sidebar column opaque, so the material
//! only shows there, with AppKit drawing its blur and its rounded edge.

use std::cell::RefCell;

use egui::{Color32, Rect};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSColor, NSColorSpace, NSGlassEffectView, NSGlassEffectViewStyle,
    NSView, NSWindowButton, NSWindowOrderingMode,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// How far the glass sits in from the window's edges and the column's.
/// Close enough that the traffic lights, which AppKit places about 16
/// points in, sit wholly inside its rounded corner.
pub const INSET: f32 = 5.0;
/// The glass's corner radius.
pub const RADIUS: f32 = 14.0;

struct Glass {
    content: Retained<NSView>,
    column: Retained<NSView>,
    material: Retained<NSGlassEffectView>,
    shown: Option<(Rect, Color32)>,
}

thread_local! {
    static GLASS: RefCell<Option<Glass>> = const { RefCell::new(None) };
}

/// Builds the glass views under the window's content. Call once, on the
/// main thread, after the window exists.
pub fn install(window: &impl HasWindowHandle) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: the handle is the live NSView winit made for this window, and
    // this runs on the main thread that owns it.
    let content: Retained<NSView> =
        unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()).expect("window view") };
    let Some(frame_view) = (unsafe { content.superview() }) else {
        return;
    };
    let sizing =
        NSAutoresizingMaskOptions::ViewHeightSizable | NSAutoresizingMaskOptions::ViewMaxXMargin;

    let column = NSView::initWithFrame(mtm.alloc(), NSRect::ZERO);
    column.setWantsLayer(true);
    column.setAutoresizingMask(sizing);
    column.setHidden(true);

    // Liquid Glass, as Music's sidebar wears it.
    let material = NSGlassEffectView::initWithFrame(mtm.alloc(), NSRect::ZERO);
    material.setStyle(NSGlassEffectViewStyle::Regular);
    material.setCornerRadius(f64::from(RADIUS));
    material.setAutoresizingMask(NSAutoresizingMaskOptions::ViewHeightSizable);
    column.addSubview(&material);

    frame_view.addSubview_positioned_relativeTo(
        &column,
        NSWindowOrderingMode::Below,
        Some(&content),
    );
    GLASS.with(|glass| {
        *glass.borrow_mut() = Some(Glass {
            content,
            column,
            material,
            shown: None,
        });
    });
}

/// Shows the app in the Dock and the app switcher, or keeps it out of
/// them while it runs without a window.
pub fn set_dock_icon(shown: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let policy = if shown {
        objc2_app_kit::NSApplicationActivationPolicy::Regular
    } else {
        objc2_app_kit::NSApplicationActivationPolicy::Accessory
    };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    if app.activationPolicy() != policy {
        app.setActivationPolicy(policy);
    }
}

/// winit's view, which the page is drawn in, for views laid over it.
pub(crate) fn content_view() -> Option<Retained<NSView>> {
    GLASS.with(|glass| glass.borrow().as_ref().map(|glass| glass.content.clone()))
}

/// Places the glass in `column`, in window points from the top left, on a
/// `window`-coloured ground, or hides it with `None`. Cheap when nothing
/// changed.
pub fn show_sidebar(column: Option<Rect>, window: Color32) {
    GLASS.with(|glass| {
        let mut glass = glass.borrow_mut();
        let Some(glass) = glass.as_mut() else {
            return;
        };
        let wanted = column.map(|rect| (rect, window));
        if glass.shown == wanted {
            return;
        }
        glass.shown = wanted;
        let Some(rect) = column else {
            glass.column.setHidden(true);
            return;
        };
        // The frame view is not flipped: its origin is the bottom left.
        let height = glass.content.frame().size.height;
        let frame = NSRect::new(
            NSPoint::new(f64::from(rect.left()), height - f64::from(rect.bottom())),
            NSSize::new(f64::from(rect.width()), f64::from(rect.height())),
        );
        glass.column.setFrame(frame);
        let inset = f64::from(INSET);
        glass.material.setFrame(NSRect::new(
            NSPoint::new(inset, inset),
            NSSize::new(
                (frame.size.width - inset).max(0.0),
                (frame.size.height - 2.0 * inset).max(0.0),
            ),
        ));
        let [r, g, b, _] = window.to_array();
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            1.0,
        );
        // SAFETY: the column is layer-backed; CALayer takes a CGColor.
        unsafe {
            let layer: Option<Retained<AnyObject>> = objc2::msg_send![&*glass.column, layer];
            if let Some(layer) = layer {
                let cg: *const CGColor = objc2::msg_send![&*color, CGColor];
                let () = objc2::msg_send![&*layer, setBackgroundColor: cg];
            }
        }
        glass.column.setHidden(false);
    });
}

/// Moves the traffic lights so the close button's centre sits `center`
/// points in from the window's top and left edges, as Music's toolbar
/// window places them, keeping AppKit's spacing between the three.
///
/// AppKit lays them out afresh on resizes and fullscreen changes, so this
/// runs every frame; it changes nothing once they are in place.
pub fn place_window_buttons(center: f32) {
    GLASS.with(|glass| {
        let glass = glass.borrow();
        let Some(glass) = glass.as_ref() else {
            return;
        };
        let Some(window) = glass.content.window() else {
            return;
        };
        let [Some(close), Some(minimize), Some(zoom)] = [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ]
        .map(|kind| window.standardWindowButton(kind)) else {
            return;
        };
        // SAFETY: AppKit's own titlebar views, read and moved on the main
        // thread that owns them.
        let Some(titlebar) = (unsafe { close.superview() }) else {
            return;
        };
        let Some(container) = (unsafe { titlebar.superview() }) else {
            return;
        };
        let Some(frame_view) = (unsafe { container.superview() }) else {
            return;
        };
        let center = f64::from(center);
        let band = 2.0 * center;
        let outer = frame_view.frame().size;
        // The buttons' container spans the band, so the buttons can sit
        // in its middle without being clipped.
        let wanted = NSRect::new(
            NSPoint::new(0.0, outer.height - band),
            NSSize::new(outer.width, band),
        );
        if container.frame() != wanted {
            container.setFrame(wanted);
        }
        let inner = NSRect::new(NSPoint::new(0.0, 0.0), wanted.size);
        if titlebar.frame() != inner {
            titlebar.setFrame(inner);
        }
        let size = close.frame().size;
        let spacing = minimize.frame().origin.x - close.frame().origin.x;
        for (index, button) in [&close, &minimize, &zoom].into_iter().enumerate() {
            let origin = NSPoint::new(
                center - size.width / 2.0 + index as f64 * spacing,
                (band - size.height) / 2.0,
            );
            if button.frame().origin != origin {
                button.setFrameOrigin(origin);
            }
        }
    });
}

/// Core Graphics' colour, only ever passed from AppKit to Core Animation.
#[repr(C)]
struct CGColor {
    _private: [u8; 0],
}

// SAFETY: matches Core Graphics' `CGColorRef`, an opaque struct pointer.
unsafe impl objc2::encode::RefEncode for CGColor {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("CGColor", &[]));
}

/// The accent colour chosen in System Settings, as sRGB.
pub fn accent_color() -> Option<[u8; 3]> {
    let color = NSColor::controlAccentColor();
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([
        channel(srgb.redComponent()),
        channel(srgb.greenComponent()),
        channel(srgb.blueComponent()),
    ])
}
