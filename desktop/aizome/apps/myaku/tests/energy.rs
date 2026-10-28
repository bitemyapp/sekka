//! The optional meter window, driven by fixture readings without hardware
//! access or signals to real processes.

use std::sync::Arc;

use aizome_kit::Program;
use aizome_kit::layout::{Size, Vec2};
use aizome_kit::testing::Harness;
use myaku::app::{Msg, Myaku, Request};
use myaku::fixture;
use myaku::model::{Frame, Lens, Meter};

fn start(frame: Frame) -> Harness<Myaku> {
    let (signals, _) = Myaku::recorder();
    Harness::new(Myaku::with_frame(
        Arc::new(frame),
        signals,
        Request { lens: Some(Lens::Energy), ..Default::default() },
    ))
}

#[test]
fn the_meter_window_is_optional_live_and_can_be_closed_or_reopened() {
    let mut h = start(fixture::workstation(100, 40));
    let main = h.window();
    assert_eq!(h.windows().len(), 1);
    let menus = h.program().menus();
    let view = menus.menus().iter().find(|m| m.title == "View").unwrap();
    assert!(view.actions().iter().any(|a| a.label == "Energy Meters…" && a.enabled));
    let status = h.find_id(main, "energy-meters").expect("clickable Energy status");
    h.click(main, status.center());
    let meters = h.program().energy_meters.unwrap();
    assert_eq!(h.title(meters), "Energy Meters");
    assert_eq!(h.program().lens, Lens::Energy);
    assert_eq!(h.texts(meters).iter().filter(|s| *s == "Counting").count(), 2, "RAPL and the discrete GPU");
    assert!(h.texts(meters).iter().any(|s| s == "Shared among applications by processor use."));
    assert!(
        h.texts(meters).iter().any(|s| s == "Measures these integrated graphics alone, not the processor package."),
        "a Ryzen desktop processor's Radeon"
    );
    h.send(Msg::EnergyMeters);
    assert_eq!(h.windows().len(), 2, "opening again reuses the meter window");

    h.send(Msg::Frame(Arc::new(fixture::frame(100, 41))));
    assert_eq!(h.program().energy_meters, Some(meters));
    assert!(!h.texts(meters).iter().any(|s| s == "Counting"));
    assert!(h.texts(meters).iter().any(|s| s == "No power reading is available for this card."));
    assert!(h.click_text(meters, "Close"));
    assert_eq!(h.windows(), vec![main]);
    assert!(h.program().energy_meters.is_none());
    h.send(Msg::EnergyMeters);
    h.request_close(h.program().energy_meters.unwrap());
    assert_eq!(h.windows(), vec![main]);
    assert!(h.program().energy_meters.is_none());
    h.send(Msg::EnergyMeters);
    h.request_close(main);
    assert!(h.windows().is_empty(), "the utility window doesn't keep Myaku open");
}

#[test]
fn an_apus_radeon_measures_the_package_without_rapl() {
    let mut f = fixture::workstation(100, 40);
    f.gpus[0].package_meter = true;
    f.gpus[0].power = Some(9.2);
    f.power.meter = Meter::Package;
    f.power.cpu = Some(9.2);
    let mut h = start(f);
    h.send(Msg::EnergyMeters);
    let meters = h.program().energy_meters.unwrap();
    let texts = h.texts(meters);
    assert_eq!(texts.iter().filter(|s| *s == "Counting").count(), 2, "the APU's Radeon and the discrete GPU");
    assert!(texts.iter().any(|s| s == "Includes the processor package; shared by processor use."));
    assert!(texts.iter().any(|s| s == "No RAPL reading is available to Myaku."));
}

#[test]
fn the_report_distinguishes_replacement_meters_and_battery_overlap() {
    let mut f = fixture::workstation(100, 40);
    f.gpus[0].package_meter = true; // An APU's Radeon.
    f.power.meter = Meter::Rapl;
    f.power.cpu = Some(60.0);
    f.power.total = Some(60.0 + f.power.graphics.unwrap());
    f.power.battery = Some(500.0);
    f.power.on_battery = true;
    let mut h = start(f.clone());
    h.send(Msg::EnergyMeters);
    let meters = h.program().energy_meters.unwrap();
    let texts = h.texts(meters);
    assert_eq!(texts.iter().filter(|s| *s == "Counting").count(), 2, "RAPL and the discrete GPU");
    assert_eq!(texts.iter().filter(|s| *s == "Not counted").count(), 2, "AMD package and battery overlap");
    assert!(texts.iter().any(|s| s == "RAPL already measures the processor package."));
    assert!(texts.iter().any(|s| s == "The processor package meter takes priority over the battery."));

    f.tick += 1;
    f.power.meter = Meter::Battery;
    f.power.cpu = None;
    f.power.total = f.power.battery;
    for g in &mut f.gpus {
        if g.integrated {
            g.power = None;
        }
    }
    h.send(Msg::Frame(Arc::new(f)));
    let texts = h.texts(meters);
    assert_eq!(texts.iter().filter(|s| *s == "Counting").count(), 1, "only the battery adds to the total");
    assert!(texts.iter().any(|s| s == "Included in battery total"));
    assert!(texts.iter().any(|s| s == "Used to estimate this card's applications; not added again."));
    assert!(texts.iter().any(|s| s == "500 W"));
}

#[test]
fn no_reading_and_uncounted_integrated_graphics_are_explained_at_small_sizes() {
    let mut h = start(Frame::default());
    h.send(Msg::EnergyMeters);
    let meters = h.program().energy_meters.unwrap();
    assert!(h.texts(meters).iter().any(|s| s == "Waiting for the first reading…"));
    let mut f = fixture::frame(100, 1);
    f.gpus[0].power = Some(5.0); // An Intel integrated reading isn't a package meter.
    h.send(Msg::Frame(Arc::new(f.clone())));
    assert!(h.texts(meters).iter().any(|s| s == "Not counted"));
    assert!(h.texts(meters).iter().any(|s| s == "Integrated graphics are not counted as a separate energy meter."));
    assert!(!h.texts(meters).iter().any(|s| s == "Counting"));
    f.tick += 1;
    f.gpus.clear();
    h.send(Msg::Frame(Arc::new(f)));
    assert!(h.texts(meters).iter().any(|s| s == "No graphics cards were detected."));
    h.resize(meters, Size::new(380.0, 320.0));
    let list = h.find_id(meters, "meter-list").unwrap();
    h.scroll(meters, list.center(), Vec2::new(0.0, 1000.0));
    assert!(h.find(meters, "No discharging-battery reading is available.").is_some());
    let close = h.find(meters, "Close").unwrap();
    assert!(close.x0 >= 0.0 && close.x1 <= 380.0 && close.y1 <= 320.0);
}
