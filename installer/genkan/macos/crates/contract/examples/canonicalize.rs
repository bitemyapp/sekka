// SPDX-License-Identifier: GPL-3.0-or-later
//! Read a document on standard input, check it as its schema says, and
//! print its canonical form and digests. The engine's tests compare their
//! own canonical bytes and digests with this output, so both languages agree
//! on every byte.
//!
//! ```sh
//! cargo run -q -p sekka-mac-contract --example canonicalize < inspection.json
//! ```
use std::io::Read;

use sekka_mac_contract::*;

fn main() {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).expect("standard input");
    let value: serde_json::Value = serde_json::from_slice(&input).expect("JSON");
    let schema = value["schema"].as_str().unwrap_or_default().to_owned();
    let relaxed = |limit| canonical::from_slice_relaxed::<serde_json::Value>(&input, limit, &schema);
    let report = match schema.as_str() {
        InspectionSchema::VALUE => {
            let i: Inspection = canonical::from_slice_relaxed(&input, limits::INSPECTION, &schema).unwrap();
            let layout = i.disk.layout_digest().unwrap();
            if let Err(e) = i.check() {
                eprintln!("inconsistent: {e}");
            }
            serde_json::json!({ "layout_digest": layout.to_string(), "canonical": String::from_utf8(canonical::to_vec(&i).unwrap()).unwrap() })
        }
        PlanSchema::VALUE => {
            let p: Plan = canonical::from_slice_relaxed(&input, limits::PLAN, &schema).unwrap();
            p.check().unwrap();
            serde_json::json!({ "digest": p.digest().unwrap().to_string(), "canonical": String::from_utf8(canonical::to_vec(&p).unwrap()).unwrap() })
        }
        EngineSchema::VALUE => {
            let m: EngineMessage = canonical::from_slice_relaxed(&input, limits::ENGINE_LINE, &schema).unwrap();
            serde_json::json!({ "canonical": String::from_utf8(canonical::to_vec(&m).unwrap()).unwrap() })
        }
        RecoverySchema::VALUE => {
            let r: RecoveryConfig = canonical::from_slice_relaxed(&input, limits::RECOVERY, &schema).unwrap();
            r.check().unwrap();
            serde_json::json!({ "canonical": String::from_utf8(canonical::to_vec(&r).unwrap()).unwrap() })
        }
        HandoffSchema::VALUE => {
            let h: Handoff = canonical::from_slice_relaxed(&input, limits::HANDOFF, &schema).unwrap();
            serde_json::json!({ "canonical": String::from_utf8(h.to_bytes().unwrap()).unwrap(), "kernel_arguments": h.kernel_arguments() })
        }
        _ => {
            let v = relaxed(limits::CATALOG).unwrap();
            serde_json::json!({ "digest": canonical::digest(&v).unwrap().to_string(), "canonical": String::from_utf8(canonical::to_vec(&v).unwrap()).unwrap() })
        }
    };
    if let Some(uuid) = std::env::args().nth(1) {
        println!("machine binding of {uuid}: {}", machine_binding(&uuid).unwrap());
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
