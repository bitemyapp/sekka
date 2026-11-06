// SPDX-License-Identifier: GPL-3.0-or-later
//! Small helpers for building AppKit views from Rust: closures as
//! target-action targets, labels, buttons, stacks and constraints, and
//! Sekka's colors as dynamic NSColors that follow the appearance.
use objc2_app_kit::NSAccessibility;
use std::cell::RefCell;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameDarkAqua, NSButton, NSColor, NSFont, NSFontWeightBold,
    NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSLayoutAttribute,
    NSLineBreakMode, NSStackView, NSStackViewDistribution, NSTextField, NSUserInterfaceLayoutOrientation,
    NSView,
};
use objc2_foundation::{NSArray, NSEdgeInsets, NSObjectProtocol, NSString};

pub struct TargetIvars {
    callback: RefCell<Box<dyn FnMut()>>,
}

define_class!(
    /// A target for target-action: calls its Rust closure. Controls hold
    /// their targets weakly, so whoever builds a control keeps its target.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SekkaActionTarget"]
    #[ivars = TargetIvars]
    pub struct Target;

    unsafe impl NSObjectProtocol for Target {}

    impl Target {
        #[unsafe(method(fire:))]
        fn fire(&self, _sender: Option<&AnyObject>) {
            if let Ok(mut callback) = self.ivars().callback.try_borrow_mut() {
                callback();
            }
        }
    }
);

impl Target {
    pub fn new(mtm: MainThreadMarker, callback: impl FnMut() + 'static) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars {
            callback: RefCell::new(Box::new(callback)),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Keeps targets alive as long as the views that use them.
#[derive(Default)]
pub struct Targets(Vec<Retained<Target>>);
impl Targets {
    pub fn keep(&mut self, t: Retained<Target>) -> Retained<Target> {
        self.0.push(t.clone());
        t
    }
}

pub fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

pub fn font(size: f64, weight: Weight) -> Retained<NSFont> {
    let w = unsafe {
        match weight {
            Weight::Regular => NSFontWeightRegular,
            Weight::Medium => NSFontWeightMedium,
            Weight::Semibold => NSFontWeightSemibold,
            Weight::Bold => NSFontWeightBold,
        }
    };
    NSFont::systemFontOfSize_weight(size, w)
}

pub fn digits(size: f64) -> Retained<NSFont> {
    NSFont::monospacedDigitSystemFontOfSize_weight(size, unsafe { NSFontWeightRegular })
}

#[derive(Clone, Copy)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

/// A label that wraps to its container's width.
pub fn text(mtm: MainThreadMarker, s: &str, size: f64, weight: Weight) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&ns(s), mtm);
    label.setFont(Some(&font(size, weight)));
    label.setSelectable(false);
    label.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    label
}

pub fn secondary(mtm: MainThreadMarker, s: &str, size: f64) -> Retained<NSTextField> {
    let label = text(mtm, s, size, Weight::Regular);
    label.setTextColor(Some(&NSColor::secondaryLabelColor()));
    label
}

/// Selectable text, for things people copy: Apple's messages, digests.
pub fn selectable(mtm: MainThreadMarker, s: &str, size: f64) -> Retained<NSTextField> {
    let label = text(mtm, s, size, Weight::Regular);
    label.setFont(Some(&digits(size)));
    label.setSelectable(true);
    label
}

pub fn button(
    mtm: MainThreadMarker,
    targets: &mut Targets,
    title: &str,
    action: impl FnMut() + 'static,
) -> Retained<NSButton> {
    let target = targets.keep(Target::new(mtm, action));
    unsafe { NSButton::buttonWithTitle_target_action(&ns(title), Some(&target), Some(sel!(fire:)), mtm) }
}

/// The window's default button: Return presses it.
pub fn primary(
    mtm: MainThreadMarker,
    targets: &mut Targets,
    title: &str,
    action: impl FnMut() + 'static,
) -> Retained<NSButton> {
    let b = button(mtm, targets, title, action);
    b.setKeyEquivalent(&ns("\r"));
    b
}

pub fn checkbox(
    mtm: MainThreadMarker,
    targets: &mut Targets,
    title: &str,
    on: bool,
    mut action: impl FnMut(bool) + 'static,
) -> Retained<NSButton> {
    let cell: std::rc::Rc<RefCell<Option<Retained<NSButton>>>> = Default::default();
    let weak = cell.clone();
    let target = targets.keep(Target::new(mtm, move || {
        if let Some(b) = weak.borrow().as_ref() {
            action(b.state() == objc2_app_kit::NSControlStateValueOn);
        }
    }));
    let b = unsafe { NSButton::checkboxWithTitle_target_action(&ns(title), Some(&target), Some(sel!(fire:)), mtm) };
    b.setState(if on {
        objc2_app_kit::NSControlStateValueOn
    } else {
        objc2_app_kit::NSControlStateValueOff
    });
    // The checkbox's own title wraps poorly; keep it on one line and let
    // the page give it room.
    *cell.borrow_mut() = Some(b.clone());
    b
}

pub fn stack(mtm: MainThreadMarker, vertical: bool, spacing: f64, views: &[&NSView]) -> Retained<NSStackView> {
    let array = NSArray::from_slice(views);
    let s = NSStackView::stackViewWithViews(&array, mtm);
    s.setOrientation(if vertical {
        NSUserInterfaceLayoutOrientation::Vertical
    } else {
        NSUserInterfaceLayoutOrientation::Horizontal
    });
    s.setAlignment(if vertical {
        NSLayoutAttribute::Leading
    } else {
        NSLayoutAttribute::CenterY
    });
    s.setSpacing(spacing);
    s.setDistribution(NSStackViewDistribution::Fill);
    s.setTranslatesAutoresizingMaskIntoConstraints(false);
    s
}

pub fn insets(s: &NSStackView, top: f64, left: f64, bottom: f64, right: f64) {
    s.setEdgeInsets(NSEdgeInsets {
        top,
        left,
        bottom,
        right,
    });
}

/// Pin `view`'s edges inside `parent`'s with insets.
pub fn pin(view: &NSView, parent: &NSView, top: f64, left: f64, bottom: f64, right: f64) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.topAnchor()
        .constraintEqualToAnchor_constant(&parent.topAnchor(), top)
        .setActive(true);
    view.leadingAnchor()
        .constraintEqualToAnchor_constant(&parent.leadingAnchor(), left)
        .setActive(true);
    parent
        .bottomAnchor()
        .constraintEqualToAnchor_constant(&view.bottomAnchor(), bottom)
        .setActive(true);
    parent
        .trailingAnchor()
        .constraintEqualToAnchor_constant(&view.trailingAnchor(), right)
        .setActive(true);
}

/// Make `view` as wide as `parent` (less insets): for wrapping labels in a
/// vertical stack.
pub fn fill_width(view: &NSView, parent: &NSView, inset: f64) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.widthAnchor()
        .constraintEqualToAnchor_constant(&parent.widthAnchor(), -inset)
        .setActive(true);
}

pub fn size(view: &NSView, width: Option<f64>, height: Option<f64>) {
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    if let Some(w) = width {
        view.widthAnchor().constraintEqualToConstant(w).setActive(true);
    }
    if let Some(h) = height {
        view.heightAnchor().constraintEqualToConstant(h).setActive(true);
    }
}

fn srgb(hex: u32, alpha: f64) -> Retained<NSColor> {
    let r = ((hex >> 16) & 0xff) as f64 / 255.0;
    let g = ((hex >> 8) & 0xff) as f64 / 255.0;
    let b = (hex & 0xff) as f64 / 255.0;
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, alpha)
}

/// A color with Sekka's Day value in light mode and its Night value in
/// dark mode (docs/brand/tokens.json).
pub fn brand(day: u32, night: u32) -> Retained<NSColor> {
    let provider = RcBlock::new(move |appearance: std::ptr::NonNull<NSAppearance>| -> std::ptr::NonNull<NSColor> {
        // SAFETY: AppKit passes a valid appearance.
        let appearance = unsafe { appearance.as_ref() };
        let names = NSArray::from_slice(&[unsafe { NSAppearanceNameDarkAqua }]);
        let dark = appearance
            .bestMatchFromAppearancesWithNames(&names)
            .is_some();
        let color = srgb(if dark { night } else { day }, 1.0);
        std::ptr::NonNull::new(Retained::autorelease_ptr(color)).expect("a color")
    });
    unsafe { NSColor::colorWithName_dynamicProvider(None, &provider) }
}

/// The crest blue: `accent` in the brand tokens.
pub fn crest() -> Retained<NSColor> {
    brand(0x1357b5, 0x7aa2f7)
}

pub fn success() -> Retained<NSColor> {
    brand(0x49602f, 0x9ece6a)
}

pub fn warning() -> Retained<NSColor> {
    brand(0x705631, 0xe0af68)
}

pub fn danger() -> Retained<NSColor> {
    brand(0xb3083a, 0xf7768e)
}

/// For views that must tell their own kind apart in tests and accessibility.
pub fn label(view: &NSView, label: &str) {
    view.setAccessibilityLabel(Some(&ns(label)));
}
