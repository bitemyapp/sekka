//! An optional live view of the meters already read by the sampler.
//! It explains which readings contribute without probing devices again.

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::app::{Msg, Myaku};
use crate::model::{self, Frame, Meter};

fn source(name: String, watts: Option<f64>, state: &str, detail: &str, counted: bool) -> Element<Msg> {
    column![
        row![
            text(name).semibold().wrap().width(Length::Fill),
            text(watts.map(model::watts).unwrap_or_else(|| "—".into())).tabular().selectable(),
        ]
        .spacing(12.0)
        .align(Align::Start)
        .width(Length::Fill),
        text(state.to_owned()).size(11.0).semibold().role(if counted { Role::Accent } else { Role::Text3 }),
        text(detail.to_owned()).size(12.0).role(Role::Text2).wrap().width(Length::Fill),
    ]
    .spacing(4.0)
    .width(Length::Fill)
    .into()
}

fn sources(f: &Frame) -> Vec<Element<Msg>> {
    let p = &f.power;
    let rapl = p.meter == Meter::Rapl;
    let mut rows = vec![source(
        "Processor package · RAPL".into(),
        p.cpu.filter(|_| rapl),
        if rapl { "Counting" } else { "Unavailable" },
        if rapl { "Shared among applications by processor use." } else { "No RAPL reading is available to Myaku." },
        rapl,
    )];
    // Match the sampler's first readable package meter (an AMD APU's).
    let package = f.gpus.iter().position(|g| g.package_meter && g.power.is_some());
    let names = model::gpu_names(&f.gpus);
    for i in model::gpu_order(&f.gpus) {
        let g = &f.gpus[i];
        let (state, detail, counted) = if g.power.is_none() {
            ("Unavailable", "No power reading is available for this card.", false)
        } else if !g.integrated {
            if p.meter == Meter::Battery {
                ("Included in battery total", "Used to estimate this card's applications; not added again.", true)
            } else {
                ("Counting", "Added to the measured total; shared by use of this card.", true)
            }
        } else if p.meter == Meter::Package && package == Some(i) {
            ("Counting", "Includes the processor package; shared by processor use.", true)
        } else if g.package_meter && rapl {
            ("Not counted", "RAPL already measures the processor package.", false)
        } else if g.driver == "amdgpu" {
            ("Not counted", "Measures these integrated graphics alone, not the processor package.", false)
        } else {
            ("Not counted", "Integrated graphics are not counted as a separate energy meter.", false)
        };
        rows.push(source(names[i].clone(), g.power, state, detail, counted));
    }
    if f.gpus.is_empty() {
        rows.push(source("Graphics cards".into(), None, "Unavailable", "No graphics cards were detected.", false));
    }
    let battery = p.meter == Meter::Battery;
    let (state, detail) = if battery {
        ("Counting", "Whole-system draw. The graphics readings are already included in this total.")
    } else if p.battery.is_some() {
        ("Not counted", "The processor package meter takes priority over the battery.")
    } else if p.on_battery {
        ("Unavailable", "The battery is discharging, but no power reading is available.")
    } else {
        ("Unavailable", "No discharging-battery reading is available.")
    };
    rows.push(source("Battery".into(), p.battery, state, detail, battery));
    rows
}

pub fn view(m: &Myaku) -> Element<Msg> {
    let f = &m.frame;
    let total = f.power.total.map(model::watts).unwrap_or_else(|| "—".into());
    let rows = if m.frames == 0 {
        vec![text("Waiting for the first reading…").role(Role::Text3).into()]
    } else {
        sources(f)
    };
    let mut items = Vec::new();
    for (i, row) in rows.into_iter().enumerate() {
        if i > 0 {
            items.push(divider().into());
        }
        items.push(row);
    }
    column![
        column![
            text("Energy Meters").size(18.0).semibold(),
            text("Sources used for the latest Energy reading.").role(Role::Text2),
            row![text("Measured total").semibold().width(Length::Fill), text(total).size(18.0).tabular()],
        ]
        .spacing(8.0)
        .padding(16.0)
        .width(Length::Fill),
        divider(),
        scroll(aizome_kit::widgets::layout::column(items).spacing(12.0).padding(16.0).width(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .id("meter-list"),
        divider(),
        row![
            text("Updates with Myaku. Application figures are estimates from device use.")
                .size(11.0)
                .role(Role::Text3)
                .wrap()
                .width(Length::Fill),
            button("Close").on_press(Msg::CloseEnergyMeters),
        ]
        .spacing(12.0)
        .padding(12.0)
        .width(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
