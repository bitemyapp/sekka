//! `tasuke bench`: time a model here, local or cloud, with the same request
//! for each, and keep the result beside the catalog's estimates.

use std::sync::atomic::AtomicBool;

use serde_json::json;
use tasuke_agent::Options;
use tasuke_agent::bench;
use tasuke_models::{Catalog, CloudModelRef, Hardware, ModelId, Paths, Speeds, Store, fit};

use crate::args::Args;
use crate::models::{Result, failed, usage};

pub fn run(words: Vec<String>) -> Result {
    let mut args = Args::new(words);
    let json = args.flag("--json");
    let given = args.positional();
    args.finish().map_err(usage)?;
    let mut o = Options::from_config();
    let paths = Paths::from_env();
    let catalog = Catalog::load_default();
    let (key, name, on) = match given.as_deref().map(|g| (g, CloudModelRef::parse(g))) {
        Some((_, Some(r))) => {
            o.use_cloud(r.provider, &r.id);
            let name = catalog.cloud.iter().find(|c| c.provider == r.provider && c.id == r.id).map(|c| c.name.clone());
            (r.key(), name.unwrap_or_else(|| r.id.clone()), r.provider.company().to_owned())
        }
        Some((id, None)) => local(&mut o, &catalog, &paths, id)?,
        None => match o.provider {
            Some(p) => {
                let r = CloudModelRef { provider: p, id: o.model.clone() };
                let name = catalog.cloud.iter().find(|c| c.provider == p && c.id == r.id).map(|c| c.name.clone());
                (r.key(), name.unwrap_or_else(|| r.id.clone()), p.company().to_owned())
            }
            None if o.model.is_empty() => return Err(usage("which model? (tasuke models lists them)")),
            None => {
                let id = o.model.clone();
                local(&mut o, &catalog, &paths, &id)?
            }
        },
    };
    if !json {
        eprintln!("Timing {name} on {on}: one answer of a few hundred words.");
    }
    o.resolve_key();
    let speed = bench::measure(&o, &AtomicBool::new(false), &mut |s| {
        if !json {
            eprintln!("{s}");
        }
    })
    .map_err(|e| failed(format!("couldn't time {name}: {e}")))?;
    let m = speed.measured(&on);
    let file = Speeds::path(&paths);
    let mut speeds = Speeds::load(&file);
    speeds.record(&key, m.clone());
    speeds.save(&file).map_err(|e| failed(format!("couldn't save the measurement: {e}")))?;
    if json {
        return crate::models::print_json(&json!({"model": key, "name": name, "measured": m}));
    }
    let prefill = m.prefill.map(|p| format!(", reading {p:.0} tokens/s")).unwrap_or_default();
    println!(
        "{name}: {:.0} tokens/s writing{prefill}, first token after {:.1} s ({} tokens).",
        m.decode,
        speed.first_token.as_secs_f64(),
        m.tokens
    );
    Ok(())
}

/// Point `o` at a downloaded catalog model on Tasuke's own server.
fn local(o: &mut Options, catalog: &Catalog, paths: &Paths, id: &str) -> Result<(String, String, String)> {
    let entry = catalog
        .get(&ModelId::new(id))
        .ok_or_else(|| failed(format!("there's no model {id:?} in the catalog (tasuke models lists them)")))?;
    if Store::from_paths(paths).installed(entry).is_none() {
        return Err(failed(format!("{} isn't downloaded: tasuke download {}", entry.name, entry.id)));
    }
    let hw = Hardware::detect();
    let fit = fit::fit(entry, &hw, fit::FitOptions::default());
    if !fit.fits() {
        return Err(failed(format!("{} doesn't fit on this machine", entry.name)));
    }
    o.use_local(entry.id.as_str());
    Ok((entry.id.as_str().to_owned(), entry.name.clone(), tasuke_models::speeds::place(&fit, &hw)))
}
