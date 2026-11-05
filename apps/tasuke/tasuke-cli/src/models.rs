//! The model subcommands: hardware, recommend, models, download, remove,
//! use, and the model server.

use std::fmt::Display;
use std::io::{IsTerminal, Write};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::json;
use tasuke_models::fit::{self, MemoryBasis};
use tasuke_models::{
    Catalog, Config, Download, DownloadEvent, DownloadOptions, Entry, Fit, GpuMemory, Hardware, Health, InstallState,
    ModelId, Paths, Recommendation, Role, Server, Store, recommend,
};

use crate::args::Args;
use crate::table::Table;

pub enum Failure {
    /// The command line was wrong.
    Usage(String),
    /// The command failed.
    Failed(String),
}

type Result<T = ()> = std::result::Result<T, Failure>;

fn failed(e: impl Display) -> Failure {
    Failure::Failed(e.to_string())
}

fn usage(e: impl Display) -> Failure {
    Failure::Usage(e.to_string())
}

pub fn run(command: &str, words: Vec<String>) -> Result {
    let mut args = Args::new(words);
    match command {
        "hardware" => {
            let json = args.flag("--json");
            args.finish().map_err(usage)?;
            hardware(json)
        }
        "recommend" => {
            let json = args.flag("--json");
            let role = args.value("--role").map_err(usage)?.map(|r| r.parse::<Role>()).transpose().map_err(usage)?;
            args.finish().map_err(usage)?;
            recommendations(role, json)
        }
        "models" => {
            let json = args.flag("--json");
            args.finish().map_err(usage)?;
            models(json)
        }
        "download" | "remove" | "use" => {
            let id = args.positional().ok_or_else(|| usage("which model? (tasuke models lists them)"))?;
            args.finish().map_err(usage)?;
            let ctx = Context::load()?;
            let entry = ctx.entry(&id)?;
            match command {
                "download" => download(&ctx, entry),
                "remove" => remove(&ctx, entry),
                _ => use_model(&ctx, entry),
            }
        }
        "server" => {
            let verb = args.positional().ok_or_else(|| usage("server needs start, stop, status or exec"))?;
            match verb.as_str() {
                "start" => {
                    let foreground = args.flag("--foreground");
                    args.finish().map_err(usage)?;
                    if foreground { server_foreground() } else { server_start() }
                }
                "stop" => {
                    args.finish().map_err(usage)?;
                    let ctx = Context::load()?;
                    ctx.server().stop().map_err(failed)?;
                    eprintln!("The model server is stopped.");
                    Ok(())
                }
                "status" => {
                    let json = args.flag("--json");
                    args.finish().map_err(usage)?;
                    server_status(json)
                }
                "exec" => {
                    let llama = args.value("--llama-server").map_err(usage)?;
                    args.finish().map_err(usage)?;
                    server_exec(llama.map(PathBuf::from))
                }
                // The unit's ExecStartPre: the API key clients read, made once.
                "key" => {
                    args.finish().map_err(usage)?;
                    let path = Context::load()?.server().make_api_key().map_err(failed)?;
                    println!("{}", path.display());
                    Ok(())
                }
                other => Err(usage(format!("unknown server command {other:?} (start, stop, status or exec)"))),
            }
        }
        other => Err(usage(format!("unknown command {other:?}"))),
    }
}

/// Settings, catalog and store, as every command but `hardware` needs them.
struct Context {
    paths: Paths,
    config: Config,
    catalog: Catalog,
    store: Store,
}

impl Context {
    fn load() -> Result<Context> {
        let paths = Paths::from_env();
        let config = Config::load(&paths).map_err(failed)?;
        let store = Store::from_paths(&paths);
        Ok(Context { paths, config, catalog: Catalog::load_default(), store })
    }

    fn entry(&self, id: &str) -> Result<&Entry> {
        self.catalog
            .get(&ModelId::new(id))
            .ok_or_else(|| failed(format!("there's no model {id:?} in the catalog (tasuke models lists them)")))
    }

    fn server(&self) -> Server {
        Server::new(&self.paths, &self.config)
    }
}

fn print_json(value: &impl serde::Serialize) -> Result {
    let text = serde_json::to_string_pretty(value).map_err(failed)?;
    println!("{text}");
    Ok(())
}

/// "Google · United States", or "H Company, from Qwen (Alibaba) · France, China".
fn lineage(e: &Entry) -> String {
    let mut s = e.maker.clone();
    let from: Vec<&str> = e.lineage.iter().map(String::as_str).filter(|l| !same_maker(l, &e.maker)).collect();
    if !from.is_empty() {
        s.push_str(&format!(", from {}", from.join(" and ")));
    }
    if !e.origin.is_empty() {
        let mut origin: Vec<&str> = Vec::new();
        for o in &e.origin {
            if !origin.contains(&o.as_str()) {
                origin.push(o);
            }
        }
        s.push_str(&format!(" · {}", origin.join(", ")));
    }
    s
}

/// Whether a lineage family is the maker's own ("Qwen (Alibaba)" for "Alibaba (Qwen)").
fn same_maker(family: &str, maker: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
    };
    let (f, m) = (words(family), words(maker));
    f.iter().any(|w| m.contains(w))
}

/// "Fits on the GPU …", "Too large: …", in a sentence.
fn fit_sentence(fit: &Fit, hw: &Hardware) -> String {
    let text = fit.describe(hw);
    if fit.fits() { format!("Fits {text}") } else { capitalize(&text) }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn hardware(json: bool) -> Result {
    let hw = Hardware::detect();
    if json {
        return print_json(&hw);
    }
    println!("Graphics");
    let mut t = Table::default();
    for (i, g) in hw.gpus.iter().enumerate() {
        let memory = match g.memory {
            GpuMemory::Dedicated { total, free } => format!("{total} of its own, {free} free"),
            GpuMemory::Unified { limit: Some(l) } => format!("shares system memory, up to {l}"),
            GpuMemory::Unified { limit: None } => "shares system memory".into(),
            GpuMemory::Unknown => "its memory isn't known".into(),
        };
        let mut about = Vec::new();
        if g.drives_display {
            about.push("drives the display".to_string());
        }
        if g.integrated {
            about.push("integrated".into());
        }
        if let Some(bw) = g.bandwidth {
            about.push(format!("{:.0} GB/s", bw.0 / 1e9));
        }
        t.row([i.to_string(), g.name.clone(), memory, about.join(", ")]);
    }
    if hw.gpus.is_empty() {
        println!("  none found: models would run on the processor");
    } else {
        print!("{}", t.render(2));
    }
    println!();
    let mut t = Table::default();
    t.row(["Memory".to_string(), format!("{} ({} available)", hw.ram.total, hw.ram.available)]);
    let mut features = Vec::new();
    if hw.cpu.flags.avx512 {
        features.push("AVX-512");
    } else if hw.cpu.flags.avx2 {
        features.push("AVX2");
    }
    let features = if features.is_empty() { String::new() } else { format!(", {}", features.join(", ")) };
    t.row([
        "Processor".to_string(),
        format!("{}: {} cores, {} threads{features}", hw.cpu.model, hw.cpu.physical_cores, hw.cpu.logical_cores),
    ]);
    let (budget, place) = best_place(&hw);
    t.row(["Room for a model".to_string(), format!("{budget}, {place}")]);
    print!("{}", t.render(0));
    for note in &hw.notes {
        println!("Note: {note}");
    }
    Ok(())
}

/// The largest budget for a model and where it is, as fit plans it.
fn best_place(hw: &Hardware) -> (tasuke_models::Bytes, String) {
    let mut best = (fit::ram_budget(hw), "in system memory, on the processor".to_string());
    // Dedicated memory first: fit places a model there before unified memory.
    let dedicated =
        hw.gpus.iter().filter_map(|g| fit::gpu_budget(g, MemoryBasis::Total).map(|b| (b, g))).max_by_key(|(b, _)| *b);
    if let Some((b, g)) = dedicated {
        let reserve = if g.drives_display { " (less what the display uses)" } else { "" };
        return (b, format!("on the {}{reserve}", g.name));
    }
    for g in &hw.gpus {
        if let Some(b) = fit::unified_budget(g, hw)
            && b > best.0
        {
            best = (b, format!("in memory the {} shares", g.name));
        }
    }
    best
}

fn role_title(role: Role) -> &'static str {
    match role {
        Role::Assistant => "Assistant: diagnosing and fixing faults, and conversation",
        Role::Desktop => "Desktop: operating the desktop from screenshots",
        Role::Fast => "Fast: quick answers",
    }
}

fn score_text(r: &Recommendation) -> String {
    match (r.role, r.score) {
        (Role::Desktop, Some(t)) => format!("{} of {} desktop tasks", t.passed, t.total),
        (_, Some(t)) => format!("fixed {} of {} faults", t.passed, t.total),
        (_, None) => "not yet evaluated".into(),
    }
}

fn recommendations(role: Option<Role>, json: bool) -> Result {
    let ctx = Context::load()?;
    let hw = Hardware::detect();
    let roles = role.map_or(Role::ALL.to_vec(), |r| vec![r]);
    let prefs = &ctx.config.recommend;
    if json {
        let mut out = serde_json::Map::new();
        for role in &roles {
            let recs: Vec<serde_json::Value> = recommend(&hw, &ctx.catalog, *role, prefs)
                .into_iter()
                .map(|r| {
                    let e = ctx.catalog.get(&r.id).expect("from the catalog");
                    let mut v = serde_json::to_value(&r).unwrap_or_default();
                    if let Some(o) = v.as_object_mut() {
                        o.insert("name".into(), json!(e.name));
                        o.insert("maker".into(), json!(e.maker));
                        o.insert("lineage".into(), json!(e.lineage));
                        o.insert("origin".into(), json!(e.origin));
                        o.insert("license".into(), json!(e.license));
                        o.insert("fit_text".into(), json!(fit_sentence(&r.fit, &hw)));
                    }
                    v
                })
                .collect();
            out.insert(role.as_str().into(), serde_json::Value::Array(recs));
        }
        return print_json(&out);
    }
    if !prefs.exclude_lineage.is_empty() {
        println!(
            "Hiding models from: {} (config.toml, [recommend] exclude_lineage)\n",
            prefs.exclude_lineage.join(", ")
        );
    }
    for (n, role) in roles.iter().enumerate() {
        if n > 0 {
            println!();
        }
        println!("{}", role_title(*role));
        let recs = recommend(&hw, &ctx.catalog, *role, prefs);
        if recs.is_empty() {
            println!("  Nothing in the catalog fits this machine for this.");
            continue;
        }
        let mut t = Table::default();
        let mut curated = None;
        for (i, r) in recs.iter().enumerate() {
            let e = ctx.catalog.get(&r.id).expect("from the catalog");
            let mark = if r.curated_default { "★" } else { " " };
            t.row([format!("{mark}{:>2}", i + 1), r.id.to_string(), e.name.clone(), lineage(e)]);
            let mut detail = vec![fit_sentence(&r.fit, &hw)];
            if let Some(s) = r.estimated_decode {
                detail.push(format!("about {s}"));
            }
            detail.push(score_text(r));
            if !r.trusted {
                detail.push("damaged something in an evaluation".into());
            }
            t.detail(detail.join("; "));
            if r.curated_default {
                curated = Some((r.id.clone(), e));
            }
        }
        print!("{}", t.render(2));
        if let Some((id, e)) = curated
            && let Some(d) = ctx.catalog.defaults.iter().find(|d| d.role == *role && d.model == id)
        {
            println!("  ★ {}", d.why);
            if let Some(i) = e.incidents.first() {
                println!("    Caution: {i}");
            }
        }
    }
    Ok(())
}

fn state_text(state: InstallState) -> String {
    match state {
        InstallState::Absent => "not downloaded".into(),
        InstallState::Partial { have, total } => format!("partly downloaded ({have} of {total})"),
        InstallState::Installed { .. } => "downloaded".into(),
    }
}

fn models(json: bool) -> Result {
    let ctx = Context::load()?;
    let hw = Hardware::detect();
    let fits = ctx.catalog.fits(&hw);
    let in_use = ctx.config.model.id.as_ref();
    if json {
        let list: Vec<serde_json::Value> = ctx
            .catalog
            .models
            .iter()
            .zip(&fits)
            .map(|(e, f)| {
                json!({
                    "id": e.id,
                    "name": e.name,
                    "maker": e.maker,
                    "lineage": e.lineage,
                    "origin": e.origin,
                    "license": e.license,
                    "roles": e.roles,
                    "evaluated": e.evaluated,
                    "download_size": e.download_size(),
                    "fit": f.fit,
                    "fit_text": fit_sentence(&f.fit, &hw),
                    "estimated_decode": f.estimated_decode,
                    "state": ctx.store.state(e),
                    "in_use": in_use == Some(&e.id),
                    "incidents": e.incidents,
                })
            })
            .collect();
        return print_json(&list);
    }
    let mut t = Table::default();
    for (e, f) in ctx.catalog.models.iter().zip(&fits) {
        let roles: Vec<&str> = e.roles.iter().map(|r| r.as_str()).collect();
        let mark = if in_use == Some(&e.id) { "▸" } else { " " };
        t.row([
            format!("{mark} {}", e.id),
            e.name.clone(),
            lineage(e),
            roles.join(", "),
            e.download_size().to_string(),
        ]);
        let mut detail = vec![fit_sentence(&f.fit, &hw)];
        if let Some(s) = f.estimated_decode {
            detail.push(format!("about {s}"));
        }
        detail.push(state_text(ctx.store.state(e)));
        if !e.evaluated {
            detail.push("not yet evaluated".into());
        }
        if !e.incidents.is_empty() {
            detail.push("damaged something in an evaluation".into());
        }
        t.detail(detail.join("; "));
    }
    print!("{}", t.render(0));
    match in_use {
        Some(id) => println!("\n▸ in use ({id}). Models are kept in {}.", ctx.store.root().display()),
        None => println!("\nNo model is in use yet: tasuke recommend suggests one, tasuke use ID picks it."),
    }
    Ok(())
}

fn download(ctx: &Context, entry: &Entry) -> Result {
    if let InstallState::Installed { size } = ctx.store.state(entry) {
        eprintln!("{} is already downloaded ({size}).", entry.id);
        return Ok(());
    }
    let tty = std::io::stderr().is_terminal();
    eprintln!(
        "Downloading {} ({}, {}) into {}",
        entry.name,
        entry.download_size(),
        entry.license,
        ctx.store.model_dir(&entry.id).display()
    );
    let handle = Download::start_with(entry, &ctx.store, DownloadOptions::from_env());
    let mut last_line = Instant::now() - Duration::from_secs(60);
    let mut drawn = false;
    let clear = |drawn: &mut bool| {
        if *drawn {
            eprint!("\r\x1b[2K");
            *drawn = false;
        }
    };
    for event in handle.events.iter() {
        match event {
            DownloadEvent::Waiting => eprintln!("Waiting for another download to finish…"),
            DownloadEvent::Started { have, total } if have.0 > 0 => {
                eprintln!("Resuming: {have} of {total} already here.")
            }
            DownloadEvent::Started { .. } => {}
            DownloadEvent::Rehashing { file, done, total } => {
                if tty {
                    eprint!("\r\x1b[2KChecking what's here of {file}: {done} of {total}");
                    drawn = true;
                }
            }
            DownloadEvent::Progress(p) => {
                let pct = if p.total.0 > 0 { p.done.0 as f64 * 100.0 / p.total.0 as f64 } else { 0.0 };
                let eta = p.eta.map(|d| format!(", {} left", duration(d))).unwrap_or_default();
                let line = format!("{pct:5.1}%  {} of {}  {}{eta}", p.done, p.total, p.rate);
                if tty {
                    eprint!("\r\x1b[2K{line}");
                    drawn = true;
                } else if last_line.elapsed() >= Duration::from_secs(10) {
                    eprintln!("{line}");
                    last_line = Instant::now();
                }
            }
            DownloadEvent::Verified { file } => {
                clear(&mut drawn);
                eprintln!("Verified {file}.");
            }
            DownloadEvent::Retrying { attempt, delay, error } => {
                clear(&mut drawn);
                eprintln!("{error}; trying again in {} (attempt {attempt})", duration(delay));
            }
            DownloadEvent::Paused | DownloadEvent::Resumed => {}
            DownloadEvent::Finished { .. } | DownloadEvent::Failed { .. } | DownloadEvent::Cancelled => {
                clear(&mut drawn)
            }
        }
        let _ = std::io::stderr().flush();
    }
    match handle.wait() {
        Ok(_) => {
            eprintln!("Downloaded {}.", entry.id);
            if ctx.config.model.id.as_ref() != Some(&entry.id) {
                eprintln!("To make it Tasuke's model: tasuke use {}", entry.id);
            }
            Ok(())
        }
        Err(e) => Err(failed(format!("{e} (running it again resumes the download)"))),
    }
}

fn duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{} h {} min", s / 3600, s % 3600 / 60)
    } else if s >= 60 {
        format!("{} min {} s", s / 60, s % 60)
    } else {
        format!("{s} s")
    }
}

fn remove(ctx: &Context, entry: &Entry) -> Result {
    let state = ctx.store.state(entry);
    if state == InstallState::Absent {
        eprintln!("{} isn't downloaded.", entry.id);
        return Ok(());
    }
    ctx.store.remove(&entry.id).map_err(failed)?;
    let freed = match state {
        InstallState::Installed { size } => size,
        InstallState::Partial { have, .. } => have,
        InstallState::Absent => tasuke_models::Bytes::ZERO,
    };
    eprintln!("Removed {} ({freed} freed).", entry.id);
    if ctx.config.model.id.as_ref() == Some(&entry.id) {
        eprintln!("It was Tasuke's model: download it again, or pick another with tasuke use ID.");
    }
    Ok(())
}

fn use_model(ctx: &Context, entry: &Entry) -> Result {
    let mut config = ctx.config.clone();
    config.model.id = Some(entry.id.clone());
    config.save(&ctx.paths).map_err(failed)?;
    eprintln!("Tasuke now uses {} ({}).", entry.name, entry.id);
    let hw = Hardware::detect();
    let fit = fit::fit(entry, &hw, fit::FitOptions { context: config.model.context, ..Default::default() });
    eprintln!("{} on this machine.", fit_sentence(&fit, &hw));
    if !fit.fits() {
        eprintln!("It won't load here: tasuke recommend suggests models that fit.");
    }
    if ctx.store.installed(entry).is_none() {
        eprintln!("It isn't downloaded yet: tasuke download {}", entry.id);
        return Ok(());
    }
    // A model downloaded since the server started isn't among its presets yet.
    let server = Server::new(&ctx.paths, &config);
    let status = server.status();
    if matches!(status.health, Health::Ok) && !status.models.iter().any(|m| m.id == entry.id.as_str()) {
        match server.restart() {
            Ok(()) => eprintln!("Restarted the model server to serve it."),
            Err(e) => eprintln!("Restart the model server to serve it ({e})."),
        }
    }
    Ok(())
}

fn server_start() -> Result {
    let ctx = Context::load()?;
    let server = ctx.server();
    match server.unit_state() {
        Some(u) if u.installed() => {}
        _ => {
            return Err(failed(format!(
                "the user unit {} isn't installed (programs.tasuke.enable = true installs it); \
                 tasuke server start --foreground runs the server here instead",
                tasuke_models::server::UNIT
            )));
        }
    }
    server.start().map_err(failed)?;
    eprintln!("The model server is starting at {}. Models load on first use.", server.url());
    Ok(())
}

/// The llama-server to run: `--llama-server`, `$TASUKE_LLAMA_SERVER`,
/// config.toml's `[server] llama_server`, else the one on the search path.
fn llama_server(given: Option<PathBuf>, config: &Config) -> PathBuf {
    given
        .or_else(|| std::env::var_os("TASUKE_LLAMA_SERVER").map(PathBuf::from))
        .or_else(|| config.server.llama_server.clone())
        .unwrap_or_else(|| PathBuf::from("llama-server"))
}

/// Presets for the installed models on this hardware, and the API key.
fn prepare(ctx: &Context) -> Result<Server> {
    let server = ctx.server();
    let hw = Hardware::detect();
    let presets = server.write_presets(&ctx.catalog, &ctx.store, &hw, &ctx.config).map_err(failed)?;
    for (id, why) in &presets.skipped {
        eprintln!("Not serving {id}: {why}");
    }
    if presets.models.is_empty() {
        eprintln!("No downloaded model to serve yet (tasuke download ID); the server starts anyway.");
    }
    server.make_api_key().map_err(failed)?;
    Ok(server)
}

fn server_exec(given: Option<PathBuf>) -> Result {
    let ctx = Context::load()?;
    let server = prepare(&ctx)?;
    let llama = llama_server(given, &ctx.config);
    let err = server.router_command(&llama).exec();
    Err(failed(format!("couldn't run {}: {err}", llama.display())))
}

fn server_foreground() -> Result {
    let ctx = Context::load()?;
    if ctx.server().unit_state().is_some_and(|u| u.running()) {
        return Err(failed("the model server's unit is already running (tasuke server stop stops it)"));
    }
    let server = prepare(&ctx)?;
    let llama = llama_server(None, &ctx.config);
    eprintln!("Serving at {} until interrupted.", server.url());
    let status =
        server.router_command(&llama).status().map_err(|e| failed(format!("couldn't run {}: {e}", llama.display())))?;
    if status.success() { Ok(()) } else { Err(failed(format!("llama-server ended: {status}"))) }
}

fn server_status(json: bool) -> Result {
    let ctx = Context::load()?;
    let server = ctx.server();
    let status = server.status();
    if json {
        return print_json(&json!({
            "status": status,
            "model": ctx.config.model.id,
            "idle_minutes": ctx.config.server.idle_minutes,
        }));
    }
    let mut t = Table::default();
    let unit = match &status.unit {
        None => "no systemd user manager to ask".to_string(),
        Some(u) if !u.installed() => format!("{} isn't installed", tasuke_models::server::UNIT),
        Some(u) => format!("{} is {} ({})", tasuke_models::server::UNIT, u.active, u.sub),
    };
    t.row(["Unit", &unit]);
    let health = match &status.health {
        Health::Ok => "answering".to_string(),
        Health::Loading => "starting".to_string(),
        Health::Down { reason } => format!("not answering ({reason})"),
    };
    t.row(["Server".to_string(), format!("{health} at {}", status.url)]);
    let model = ctx.config.model.id.as_ref().map_or("none chosen (tasuke use ID)".to_string(), ToString::to_string);
    t.row(["Model".to_string(), model]);
    let idle = match ctx.config.server.idle_minutes {
        0 => "never".to_string(),
        m => format!("after {m} min idle"),
    };
    t.row(["Unloads".to_string(), idle]);
    print!("{}", t.render(0));
    if !status.models.is_empty() {
        println!("\nServing");
        let mut t = Table::default();
        for m in &status.models {
            let state = format!("{:?}", m.status).to_lowercase();
            let failed = if m.failed { " (its last load failed)" } else { "" };
            t.row([m.id.clone(), format!("{state}{failed}")]);
        }
        print!("{}", t.render(2));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lineage_names_the_maker_and_where_it_comes_from() {
        let catalog = Catalog::load_default();
        let get = |id: &str| catalog.get(&ModelId::new(id)).unwrap();
        for e in &catalog.models {
            let text = lineage(e);
            assert!(text.starts_with(&e.maker), "{text}");
            assert!(e.origin.iter().all(|o| text.contains(o.as_str())), "{text}");
        }
        assert_eq!(lineage(get("qwen3.8-27b")), "Alibaba (Qwen) · China");
        assert_eq!(lineage(get("holo4-27b")), "H Company, from Qwen (Alibaba) · France, China");
    }

    #[test]
    fn durations_read_plainly() {
        assert_eq!(duration(Duration::from_secs(42)), "42 s");
        assert_eq!(duration(Duration::from_secs(125)), "2 min 5 s");
        assert_eq!(duration(Duration::from_secs(7260)), "2 h 1 min");
    }
}
