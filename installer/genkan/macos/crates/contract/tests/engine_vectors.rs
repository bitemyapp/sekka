// SPDX-License-Identifier: GPL-3.0-or-later
//! The engine's vectors, read by the contract itself.
//!
//! installer/genkan/macos/engine/tests/vectors holds documents and
//! transcripts that the Python engine produced against its fake Mac, and
//! expected.json, the canonical bytes and digests this crate computed for
//! those documents. The engine's tests check that Python writes the same
//! bytes; these tests check that the documents are valid here, still give
//! the recorded bytes and digests, and that every transcript is one the
//! helper's `Transcript` accepts, ending as recorded.
use std::path::{Path, PathBuf};

use sekka_mac_contract::*;
use serde_json::Value;

fn vectors() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engine/tests/vectors")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(vectors().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn expected() -> Value {
    serde_json::from_slice(&read("expected.json")).unwrap()
}

#[test]
fn documents_have_the_recorded_bytes_and_digests() {
    let expected = expected();
    let documents = expected["documents"].as_object().unwrap();
    assert!(documents.len() >= 6);
    for (name, want) in documents {
        let input = read(name);
        let value: Value = serde_json::from_slice(&input).unwrap();
        let schema = value["schema"].as_str().unwrap_or_default().to_owned();
        let bytes = match schema.as_str() {
            InspectionSchema::VALUE => {
                let i: Inspection =
                    canonical::from_slice_relaxed(&input, limits::INSPECTION, &schema).unwrap();
                i.check().unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(
                    i.disk.layout_digest().unwrap().to_string(),
                    want["layout_digest"],
                    "{name}"
                );
                canonical::to_vec(&i).unwrap()
            }
            PlanSchema::VALUE => {
                let p: Plan = canonical::from_slice_relaxed(&input, limits::PLAN, &schema).unwrap();
                p.check().unwrap_or_else(|e| panic!("{name}: {e}"));
                canonical::to_vec(&p).unwrap()
            }
            RecoverySchema::VALUE => {
                let r: RecoveryConfig =
                    canonical::from_slice_relaxed(&input, limits::RECOVERY, &schema).unwrap();
                r.check().unwrap_or_else(|e| panic!("{name}: {e}"));
                canonical::to_vec(&r).unwrap()
            }
            HandoffSchema::VALUE => {
                let h: Handoff =
                    canonical::from_slice_relaxed(&input, limits::HANDOFF, &schema).unwrap();
                let bytes = h.to_bytes().unwrap();
                assert_eq!(Handoff::parse(&bytes).unwrap(), h);
                assert_eq!(h.kernel_arguments(), want["kernel_arguments"], "{name}");
                bytes
            }
            _ => canonical::to_vec(&value).unwrap(),
        };
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            want["canonical"],
            "{name}"
        );
        assert_eq!(Digest::of(&bytes).to_string(), want["digest"], "{name}");
    }
}

#[test]
fn the_profile_digest_is_the_recorded_one() {
    // Plans bind the profile by canonical::digest of the typed profile; the
    // engine computes it from the same file's JSON.
    assert_eq!(
        canonical::digest(&j514s()).unwrap().to_string(),
        expected()["profile_digest"]
    );
}

#[test]
fn machine_bindings_are_the_recorded_ones() {
    let expected = expected();
    for (platform, binding) in expected["machine_bindings"].as_object().unwrap() {
        assert_eq!(machine_binding(platform).unwrap().to_string(), *binding);
    }
}

#[test]
fn every_engine_transcript_is_accepted() {
    let transcripts: Value = serde_json::from_slice(&read("transcripts.json")).unwrap();
    let transcripts = transcripts.as_object().unwrap();
    assert!(transcripts.len() >= 5);
    for (name, meta) in transcripts {
        let mut t = match meta["mode"].as_str().unwrap() {
            "inspect" => Transcript::inspect(),
            "startup-disk" => {
                Transcript::startup_disk(meta["plan"].as_str().unwrap().parse().unwrap())
            }
            _ => Transcript::execute(
                meta["plan"].as_str().unwrap().parse().unwrap(),
                meta["shrinks"].as_bool().unwrap(),
            ),
        };
        let data = read(name);
        for (n, line) in data
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .enumerate()
        {
            t.accept(line)
                .unwrap_or_else(|e| panic!("{name} line {}: {e}", n + 1));
        }
        assert_eq!(
            format!("{:?}", t.outcome().unwrap()),
            meta["outcome"].as_str().unwrap(),
            "{name}"
        );
    }
}

#[test]
fn the_happy_path_runs_every_macos_stage_with_the_promised_evidence() {
    let data = read("execute-happy.jsonl");
    let mut verified = Vec::new();
    for line in data.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let m: EngineMessage =
            canonical::from_slice(line, limits::ENGINE_LINE, EngineSchema::VALUE).unwrap();
        if let EngineBody::Checkpoint {
            stage,
            verification,
            ..
        } = m.message
        {
            verified.push((stage, verification));
        }
    }
    let stages: Vec<Stage> = verified.iter().map(|(s, _)| *s).collect();
    assert_eq!(stages, macos_stages(true));
    for (stage, verification) in verified {
        let want = match stage {
            Stage::Firmware | Stage::Esp | Stage::Root | Stage::Recovery => Verification::ReadBack,
            Stage::StartupDisk => Verification::PolicyReadBack,
            _ => Verification::Inspected,
        };
        assert_eq!(verification, want, "{stage:?}");
    }
}

#[test]
fn the_startup_disk_retry_runs_that_stage_and_nothing_else() {
    let data = read("startup-disk-retry.jsonl");
    let mut intents = Vec::new();
    let mut checkpoints = Vec::new();
    for line in data.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let m: EngineMessage =
            canonical::from_slice(line, limits::ENGINE_LINE, EngineSchema::VALUE).unwrap();
        match m.message {
            EngineBody::Hello { mode, .. } => assert_eq!(mode, EngineMode::StartupDisk),
            EngineBody::Intent { stage, .. } => intents.push(stage),
            EngineBody::Checkpoint {
                stage,
                verification,
                ..
            } => checkpoints.push((stage, verification)),
            _ => {}
        }
    }
    assert_eq!(intents, vec![Stage::StartupDisk]);
    assert_eq!(
        checkpoints,
        vec![(Stage::StartupDisk, Verification::PolicyReadBack)]
    );
}
