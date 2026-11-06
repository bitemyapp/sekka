// SPDX-License-Identifier: GPL-3.0-or-later
//! The views AppKit doesn't have: Sekka's crest, the disk with its
//! draggable divider, and the marks beside steps and checks.
use objc2_app_kit::NSAccessibility;
use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibilitySliderRole, NSBezierPath, NSColor, NSCompositingOperation, NSCursor, NSEvent,
    NSGraphicsContext, NSLineCapStyle, NSLineJoinStyle, NSView,
};
use objc2_foundation::{NSCopying, NSPoint, NSRect, NSSize, NSString};

use crate::ui;

fn point(x: f64, y: f64) -> NSPoint {
    NSPoint::new(x, y)
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

define_class!(
    /// Sekka's crest (docs/brand): six circles of radius 76, 36 from the
    /// middle of a 256 grid, with a six-armed crystal and a hollow hexagon
    /// cut through them. Drawn, not shipped as an image, so it is sharp at
    /// every scale and takes the crest blue for the appearance.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaCrestView"]
    pub struct CrestView;

    impl CrestView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let b = self.bounds();
            let s = b.size.width.min(b.size.height) / 256.0;
            let p = |x: f64, y: f64| point(b.origin.x + x * s, b.origin.y + y * s);
            let Some(context) = NSGraphicsContext::currentContext() else { return };
            context.saveGraphicsState();
            ui::crest().setFill();
            for i in 0..6 {
                let a = std::f64::consts::PI / 3.0 * i as f64 + std::f64::consts::PI / 6.0;
                let (cx, cy) = (128.0 + 36.0 * a.cos(), 128.0 + 36.0 * a.sin());
                NSBezierPath::bezierPathWithOvalInRect(rect(
                    b.origin.x + (cx - 76.0) * s,
                    b.origin.y + (cy - 76.0) * s,
                    152.0 * s,
                    152.0 * s,
                ))
                .fill();
            }
            // The crystal is cut out of the ring.
            context.setCompositingOperation(NSCompositingOperation::DestinationOut);
            let arms = NSBezierPath::bezierPath();
            for (x0, y0, x1, y1) in CRYSTAL {
                arms.moveToPoint(p(x0, y0));
                arms.lineToPoint(p(x1, y1));
            }
            arms.setLineWidth(12.0 * s);
            arms.setLineCapStyle(NSLineCapStyle::Round);
            NSColor::blackColor().setStroke();
            arms.stroke();
            // The hexagon at the centre is filled back in, then outlined out.
            let hex = NSBezierPath::bezierPath();
            hex.moveToPoint(p(128.0, 109.0));
            for (x, y) in [(111.55, 118.5), (111.55, 137.5), (128.0, 147.0), (144.45, 137.5), (144.45, 118.5)] {
                hex.lineToPoint(p(x, y));
            }
            hex.closePath();
            context.setCompositingOperation(NSCompositingOperation::SourceOver);
            ui::crest().setFill();
            hex.fill();
            context.setCompositingOperation(NSCompositingOperation::DestinationOut);
            hex.setLineWidth(10.0 * s);
            hex.setLineJoinStyle(NSLineJoinStyle::Round);
            hex.stroke();
            context.restoreGraphicsState();
        }
    }
);

/// The crystal's arms and branches, from docs/brand/logos/sekka-crest-blue.svg.
const CRYSTAL: [(f64, f64, f64, f64); 18] = [
    (128.0, 109.0, 128.0, 44.0),
    (128.0, 68.0, 110.68, 58.0),
    (128.0, 68.0, 145.32, 58.0),
    (111.55, 118.5, 55.25, 86.0),
    (76.04, 98.0, 58.72, 108.0),
    (76.04, 98.0, 76.04, 78.0),
    (111.55, 137.5, 55.25, 170.0),
    (76.04, 158.0, 76.04, 178.0),
    (76.04, 158.0, 58.72, 148.0),
    (128.0, 147.0, 128.0, 212.0),
    (128.0, 188.0, 145.32, 198.0),
    (128.0, 188.0, 110.68, 198.0),
    (144.45, 137.5, 200.75, 170.0),
    (179.96, 158.0, 197.28, 148.0),
    (179.96, 158.0, 179.96, 178.0),
    (144.45, 118.5, 200.75, 86.0),
    (179.96, 98.0, 179.96, 78.0),
    (179.96, 98.0, 197.28, 108.0),
];

impl CrestView {
    pub fn new(mtm: MainThreadMarker, side: f64) -> Retained<Self> {
        let this = Self::alloc(mtm);
        let view: Retained<Self> = unsafe { msg_send![this, initWithFrame: rect(0.0, 0.0, side, side)] };
        // Cut-outs need a layer of their own to cut through.
        view.setWantsLayer(true);
        ui::size(&view, Some(side), Some(side));
        view.setAccessibilityElement(true);
        ui::label(&view, "Sekka");
        view
    }
}

/// What the disk bar shows, in bytes.
#[derive(Clone, Debug, Default)]
pub struct DiskLayout {
    pub total: u64,
    /// Before the macOS container: iBoot's system container and anything
    /// else.
    pub before: u64,
    /// macOS's container as it is now, and how much of it is used.
    pub macos: u64,
    pub macos_used: u64,
    /// Free space right after the container that joins Sekka's.
    pub adjacent_free: u64,
    pub size: u64,
    pub min: u64,
    pub max: u64,
}

type OnChange = Box<dyn FnMut(u64)>;

pub struct DiskBarIvars {
    layout: RefCell<DiskLayout>,
    on_change: RefCell<Option<OnChange>>,
    dragging: Cell<bool>,
}

/// The bar's geometry: its height and the drag handle's reach.
const BAR: f64 = 30.0;
const GRAB: f64 = 8.0;
const STEP_SMALL: u64 = 1_000_000_000;
const STEP_LARGE: u64 = 10_000_000_000;

define_class!(
    /// The internal disk as a bar: what macOS uses, its free space, Sekka's
    /// share and the rest. The edge between macOS and Sekka is a divider
    /// to drag (or move with the arrow keys; Shift moves ten times as far),
    /// and to VoiceOver it is a slider.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaDiskBar"]
    #[ivars = DiskBarIvars]
    pub struct DiskBar;

    impl DiskBar {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let l = self.ivars().layout.borrow().clone();
            if l.total == 0 {
                return;
            }
            let b = self.bounds();
            let bar = rect(b.origin.x + 1.0, b.origin.y + (b.size.height - BAR) / 2.0, b.size.width - 2.0, BAR);
            let x = |bytes: u64| bar.origin.x + bar.size.width * bytes as f64 / l.total as f64;
            let segment = |from: u64, to: u64, color: &NSColor| {
                let (x0, x1) = (x(from), x(to).max(x(from) + 1.0));
                color.setFill();
                NSBezierPath::fillRect(rect(x0, bar.origin.y, x1 - x0, bar.size.height));
            };
            let macos_end = l.before + l.macos + l.adjacent_free;
            let sekka_start = macos_end.saturating_sub(l.size);
            segment(0, l.total, &NSColor::quaternaryLabelColor());
            segment(0, l.before, &NSColor::tertiaryLabelColor());
            segment(l.before, l.before + l.macos_used, &NSColor::secondaryLabelColor());
            segment(l.before + l.macos_used, sekka_start, &NSColor::tertiaryLabelColor().colorWithAlphaComponent(0.45));
            segment(sekka_start, macos_end, &ui::crest());
            segment(macos_end, l.total, &NSColor::tertiaryLabelColor());
            // The divider.
            let dx = x(sekka_start);
            NSColor::labelColor().setFill();
            NSBezierPath::fillRect(rect(dx - 1.5, bar.origin.y - 5.0, 3.0, bar.size.height + 10.0));
            let focused = self
                .window()
                .and_then(|w| w.firstResponder())
                .is_some_and(|r| std::ptr::eq(Retained::as_ptr(&r).cast::<AnyObject>(), (self as *const Self).cast::<AnyObject>()));
            if focused {
                NSColor::keyboardFocusIndicatorColor().setStroke();
                let ring = NSBezierPath::bezierPathWithRect(rect(bar.origin.x - 2.0, bar.origin.y - 2.0, bar.size.width + 4.0, bar.size.height + 4.0));
                ring.setLineWidth(2.0);
                ring.stroke();
            }
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            let r = self.handle_rect();
            // Its replacement needs macOS 15; this app supports 14.
            #[allow(deprecated)]
            let cursor = NSCursor::resizeLeftRightCursor();
            self.addCursorRect_cursor(r, &cursor);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            let r = self.handle_rect();
            let near = (p.x - (r.origin.x + r.size.width / 2.0)).abs() <= GRAB * 2.0;
            self.ivars().dragging.set(true);
            if let Some(w) = self.window() {
                w.makeFirstResponder(Some(self));
            }
            if !near {
                self.drag_to(p.x);
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            if self.ivars().dragging.get() {
                let p = self.convertPoint_fromView(event.locationInWindow(), None);
                self.drag_to(p.x);
            }
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.ivars().dragging.set(false);
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let step = if event.modifierFlags().contains(objc2_app_kit::NSEventModifierFlags::Shift) {
                STEP_LARGE
            } else {
                STEP_SMALL
            };
            match event.keyCode() {
                // Left makes Sekka bigger (the divider moves into macOS),
                // right makes it smaller, as the picture moves.
                123 => self.nudge(step as i64),
                124 => self.nudge(-(step as i64)),
                _ => unsafe {
                    let _: () = msg_send![super(self), keyDown: event];
                },
            }
        }

        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            true
        }

        #[unsafe(method_id(accessibilityRole))]
        fn accessibility_role(&self) -> Retained<NSString> {
            unsafe { NSAccessibilitySliderRole }.copy()
        }

        #[unsafe(method_id(accessibilityLabel))]
        fn accessibility_label(&self) -> Retained<NSString> {
            NSString::from_str("Space for Sekka")
        }

        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Retained<NSString> {
            let l = self.ivars().layout.borrow();
            NSString::from_str(&format!(
                "{} for Sekka; macOS keeps {}",
                bytes(l.size),
                bytes((l.macos + l.adjacent_free).saturating_sub(l.size))
            ))
        }

        #[unsafe(method(accessibilityPerformIncrement))]
        fn accessibility_increment(&self) -> bool {
            self.nudge(STEP_SMALL as i64);
            true
        }

        #[unsafe(method(accessibilityPerformDecrement))]
        fn accessibility_decrement(&self) -> bool {
            self.nudge(-(STEP_SMALL as i64));
            true
        }
    }
);

impl DiskBar {
    pub fn new(mtm: MainThreadMarker, layout: DiskLayout, on_change: impl FnMut(u64) + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DiskBarIvars {
            layout: RefCell::new(layout),
            on_change: RefCell::new(Some(Box::new(on_change))),
            dragging: Cell::new(false),
        });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, 400.0, BAR + 12.0)] };
        ui::size(&view, None, Some(BAR + 12.0));
        view
    }

    fn handle_rect(&self) -> NSRect {
        let l = self.ivars().layout.borrow();
        let b = self.bounds();
        if l.total == 0 {
            return rect(0.0, 0.0, 0.0, 0.0);
        }
        let w = b.size.width - 2.0;
        let sekka_start = (l.before + l.macos + l.adjacent_free).saturating_sub(l.size);
        let x = b.origin.x + 1.0 + w * sekka_start as f64 / l.total as f64;
        rect(x - GRAB, b.origin.y, GRAB * 2.0, b.size.height)
    }

    fn drag_to(&self, x: f64) {
        let size = {
            let l = self.ivars().layout.borrow();
            if l.total == 0 {
                return;
            }
            let b = self.bounds();
            let w = b.size.width - 2.0;
            let at = ((x - b.origin.x - 1.0) / w).clamp(0.0, 1.0) * l.total as f64;
            let end = (l.before + l.macos + l.adjacent_free) as f64;
            let raw = (end - at).max(0.0) as u64;
            // Whole gigabytes are easier to aim for than bytes.
            (raw + STEP_SMALL / 2) / STEP_SMALL * STEP_SMALL
        };
        self.change(size);
    }

    fn nudge(&self, delta: i64) {
        let size = self.ivars().layout.borrow().size as i64 + delta;
        self.change(size.max(0) as u64);
    }

    fn change(&self, size: u64) {
        let size = {
            let mut l = self.ivars().layout.borrow_mut();
            l.size = size.clamp(l.min, l.max.max(l.min));
            l.size
        };
        self.setNeedsDisplay(true);
        if let Some(w) = self.window() {
            w.invalidateCursorRectsForView(self);
        }
        if let Some(f) = self.ivars().on_change.borrow_mut().as_mut() {
            f(size);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// A numbered step not reached yet.
    Upcoming(u8),
    /// The step the person is on.
    Current(u8),
    Done,
    Good,
    Warning,
    Bad,
    /// Nothing yet (a check that hasn't run).
    Empty,
}

pub struct MarkIvars {
    mark: Cell<Mark>,
}

define_class!(
    /// The round mark beside a journey step or a check: a number, a tick,
    /// an exclamation mark or a cross. Every mark also has its words beside
    /// it; color is never the only signal.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaMarkView"]
    #[ivars = MarkIvars]
    pub struct MarkView;

    impl MarkView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let b = self.bounds();
            let d = b.size.width.min(b.size.height) - 2.0;
            let c = rect(b.origin.x + 1.0, b.origin.y + 1.0, d, d);
            let mid = |fx: f64, fy: f64| point(c.origin.x + d * fx, c.origin.y + d * fy);
            let mark = self.ivars().mark.get();
            let (fill, ring): (Option<Retained<NSColor>>, Option<Retained<NSColor>>) = match mark {
                Mark::Upcoming(_) => (None, Some(NSColor::tertiaryLabelColor())),
                Mark::Current(_) | Mark::Done => (Some(ui::crest()), None),
                Mark::Good => (Some(ui::success()), None),
                Mark::Warning => (Some(ui::warning()), None),
                Mark::Bad => (Some(ui::danger()), None),
                Mark::Empty => (None, Some(NSColor::quaternaryLabelColor())),
            };
            let circle = NSBezierPath::bezierPathWithOvalInRect(c);
            if let Some(f) = fill {
                f.setFill();
                circle.fill();
            }
            if let Some(r) = ring {
                r.setStroke();
                let inner = NSBezierPath::bezierPathWithOvalInRect(rect(c.origin.x + 0.75, c.origin.y + 0.75, d - 1.5, d - 1.5));
                inner.setLineWidth(1.5);
                inner.stroke();
            }
            let glyph = NSBezierPath::bezierPath();
            glyph.setLineWidth((d / 9.0).max(1.6));
            glyph.setLineCapStyle(NSLineCapStyle::Round);
            glyph.setLineJoinStyle(NSLineJoinStyle::Round);
            match mark {
                Mark::Done | Mark::Good => {
                    glyph.moveToPoint(mid(0.28, 0.52));
                    glyph.lineToPoint(mid(0.44, 0.68));
                    glyph.lineToPoint(mid(0.72, 0.34));
                }
                Mark::Warning => {
                    glyph.moveToPoint(mid(0.5, 0.26));
                    glyph.lineToPoint(mid(0.5, 0.56));
                    glyph.moveToPoint(mid(0.5, 0.74));
                    glyph.lineToPoint(mid(0.5, 0.745));
                }
                Mark::Bad => {
                    glyph.moveToPoint(mid(0.33, 0.33));
                    glyph.lineToPoint(mid(0.67, 0.67));
                    glyph.moveToPoint(mid(0.67, 0.33));
                    glyph.lineToPoint(mid(0.33, 0.67));
                }
                _ => {}
            }
            ui::on_fill().setStroke();
            glyph.stroke();
        }
    }
);

impl MarkView {
    pub fn new(mtm: MainThreadMarker, side: f64, mark: Mark) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(MarkIvars { mark: Cell::new(mark) });
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, side, side)] };
        ui::size(&view, Some(side), Some(side));
        view
    }

    pub fn set_mark(&self, mark: Mark) {
        if self.ivars().mark.get() != mark {
            self.ivars().mark.set(mark);
            self.setNeedsDisplay(true);
        }
    }
}

define_class!(
    /// A plain view with its origin at the top left, so documents in a
    /// scroll view start at the top.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaFlippedView"]
    pub struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FlippedView {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm);
        unsafe { msg_send![this, initWithFrame: rect(0.0, 0.0, 100.0, 100.0)] }
    }
}

/// Bytes as Finder counts them (decimal units), for accessibility values.
pub fn bytes(n: u64) -> String {
    if n < 1000 {
        return format!("{n} bytes");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = n as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if value >= 100.0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}
