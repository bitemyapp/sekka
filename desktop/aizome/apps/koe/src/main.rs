//! `aizome-koe`: Koe's window, with no command (`--page vocabulary` for
//! its other page); the session's dictation service (`daemon`); and the
//! commands shortcuts, scripts and agents use.
use anyhow::{Context, Result, bail, ensure};
use clap::{Parser, Subcommand};
use koe::{
    app::{Koe, Page, Paths},
    config::{self, Config},
    instance::{self, Claim},
    ipc,
    protocol::{Action, Request, Response, VocabularyReply, VocabularyRequest as V},
    service::{self, Service, Session},
    vocabulary::{self, Example, Library, Pack, Store, Word},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
use tokio::{io::BufReader, net::UnixStream};

#[derive(Parser)]
#[command(name = "aizome-koe", version, about = "Koe 声 — Aizome's dictation. With no command, opens Koe's window.")]
struct Cli {
    #[arg(long, global = true, help = "Configuration file (default: $XDG_CONFIG_HOME/aizome/koe.toml)")]
    config: Option<PathBuf>,
    #[arg(long, global = true, help = "Override the private daemon socket")]
    socket: Option<PathBuf>,
    /// The window's page: dictation or vocabulary.
    #[arg(long, value_name = "PAGE")]
    page: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Run the session daemon. Recording starts only on request.
    Daemon {
        /// Run without a StatusNotifierItem (for headless sessions).
        #[arg(long)]
        no_tray: bool,
    },
    /// Turn dictation on, now and at each login, as Koe's switch does.
    Enable,
    /// Turn dictation off, now and at login.
    Disable,
    /// Succeed only while dictation is on (the service's start condition).
    Enabled,
    /// Manage shareable vocabulary packs without opening a window.
    Packs {
        #[command(subcommand)]
        command: PackCommand,
    },
    /// Add, change or remove a pack's words and approved corrections.
    Words {
        #[command(subcommand)]
        command: WordCommand,
    },
    /// Review or delete saved teaching examples (export: aizome-koe packs export-examples).
    Examples {
        #[command(subcommand)]
        command: ExampleCommand,
    },
    /// Control dictation; start/stop can be bound to press/release for push-to-talk.
    Record {
        #[arg(value_enum)]
        action: Action,
    },
    /// Print machine-readable daemon status.
    Status,
    /// Stream newline-delimited JSON status for the Aizome shell or other bars.
    Watch {
        #[arg(long, help = "Emit Waybar custom-module JSON")]
        waybar: bool,
    },
    /// Transcribe a mono 16 kHz WAV to stdout, without clipboard changes.
    Transcribe {
        input: PathBuf,
        #[arg(long)]
        model: Option<PathBuf>,
    },
    /// Show, validate, or create configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Download verified local models and manage resident memory.
    Model {
        #[command(subcommand)]
        command: ModelCommand,
    },
    /// Check configuration, model, runtime, and desktop dependencies.
    Doctor,
}
#[derive(Subcommand)]
enum ConfigCommand {
    Show,
    Check,
    Init,
}
// Packs and words are named by ID or, when unambiguous, by name or spelling.
#[derive(Subcommand)]
enum PackCommand {
    /// Print the whole library: packs, words, corrections and active packs.
    List,
    /// Print a pack, whether it is active, and each word's saved examples.
    Show {
        pack: String,
    },
    /// Create a pack and print it. It stays inactive unless --enable is given.
    Create {
        name: String,
        #[arg(long, default_value = "")]
        description: String,
        /// Stable ID (default: derived from the name).
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        enable: bool,
    },
    /// Rename a pack or change its description, and print it.
    Edit {
        pack: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Remove a pack. Delete its saved examples first.
    Remove {
        pack: String,
    },
    /// Import a shared .koe.json pack; it starts inactive.
    Import {
        path: PathBuf,
    },
    /// Write a pack to a new .koe.json file. Recordings are never included.
    Export {
        pack: String,
        path: PathBuf,
    },
    /// Use a pack's words and corrections across the desktop.
    Enable {
        pack: String,
    },
    Disable {
        pack: String,
    },
    /// Copy a pack's saved examples to a new folder: audio/*.wav and manifest.jsonl.
    ExportExamples {
        pack: String,
        path: PathBuf,
    },
}
#[derive(Subcommand)]
enum WordCommand {
    /// Add a word to a pack and print it.
    Add {
        pack: String,
        /// The written form dictation should produce.
        spelling: String,
        /// A misrecognition to replace with SPELLING; repeat for more.
        #[arg(long = "alias", value_name = "ALIAS")]
        aliases: Vec<String>,
        /// Stable ID (default: derived from the spelling).
        #[arg(long)]
        id: Option<String>,
    },
    /// Change a word's written form or approved corrections, and print it.
    Edit {
        pack: String,
        word: String,
        #[arg(long)]
        spelling: Option<String>,
        #[arg(long = "add-alias", value_name = "ALIAS")]
        add: Vec<String>,
        #[arg(long = "remove-alias", value_name = "ALIAS")]
        remove: Vec<String>,
    },
    /// Remove a word. Delete its saved examples first.
    Remove { pack: String, word: String },
}
#[derive(Subcommand)]
enum ExampleCommand {
    /// Print saved examples (all packs' by default), newest first: complete
    /// transcripts, raw recognition, provenance and each recording's path.
    List {
        pack: Option<String>,
        #[arg(long, requires = "pack")]
        word: Option<String>,
    },
    /// Delete a saved example's recording and transcript.
    Delete { id: String },
}
#[derive(Subcommand)]
enum ModelCommand {
    List,
    Download {
        #[arg(default_value = "base")]
        name: String,
        #[arg(long)]
        directory: Option<PathBuf>,
    },
    Verify {
        name: String,
        #[arg(long)]
        directory: Option<PathBuf>,
    },
    /// Delete a downloaded model.
    Remove {
        name: String,
        #[arg(long)]
        directory: Option<PathBuf>,
    },
    Unload,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();
    let result = (|| -> Result<()> {
        let path = cli.config.clone().map(Ok).unwrap_or_else(config::config_path)?;
        // Packs, models and the configuration work outside a session.
        let needs_socket = !matches!(
            cli.command,
            Some(
                Command::Enabled
                    | Command::Packs { .. }
                    | Command::Words { .. }
                    | Command::Examples { .. }
                    | Command::Transcribe { .. }
                    | Command::Config { .. }
                    | Command::Doctor
                    | Command::Model {
                        command: ModelCommand::List
                            | ModelCommand::Download { .. }
                            | ModelCommand::Verify { .. }
                            | ModelCommand::Remove { .. }
                    }
            )
        );
        let socket = match (&cli.socket, ipc::socket_path()) {
            (Some(socket), _) => socket.clone(),
            (None, Ok(socket)) => socket,
            (None, Err(error)) if needs_socket => return Err(error),
            (None, Err(_)) => PathBuf::new(),
        };
        // The session's unit runs the default configuration and socket.
        let session = Session {
            unit: path == config::config_path()? && ipc::socket_path().is_ok_and(|s| s == socket),
            config: path,
            socket,
        };
        match cli.command {
            None => window(session, cli.page.as_deref()),
            // Quietly: systemd only wants the answer.
            Some(Command::Enabled) => std::process::exit(if service::enabled(&session.config) { 0 } else { 1 }),
            Some(command) => tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?
                .block_on(run(session, command)),
        }
    })();
    if let Err(e) = result {
        eprintln!("aizome-koe: {e:#}");
        std::process::exit(1);
    }
}

/// Koe's window: the one already open, shown, or a new one.
fn window(session: Session, page: Option<&str>) -> Result<()> {
    let page = page.map(|p| Page::parse(p).context("--page is dictation or vocabulary")).transpose()?;
    let paths = Paths::user(session.config.clone(), session.socket.clone())?;
    let mut app = Koe::new(paths, Arc::new(session));
    if let Some(page) = page {
        app.page = page;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match instance::claim("koe", &args) {
        Claim::Handed => return Ok(()),
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::try_run(app).map(|_| ()).map_err(|e| anyhow::anyhow!("cannot open Koe's window: {e}"))
}

/// What a window this program opens is told: its paths, when they aren't
/// the defaults.
fn window_args(session: &Session, page: Page) -> Vec<String> {
    let mut args = vec![];
    if !session.unit {
        args.extend([
            "--config".into(),
            session.config.display().to_string(),
            "--socket".into(),
            session.socket.display().to_string(),
        ]);
    }
    args.extend(["--page".into(), page.arg().into()]);
    args
}

async fn run(session: Session, command: Command) -> Result<()> {
    let path = session.config.clone();
    let socket = session.socket.clone();
    match command {
        Command::Daemon { no_tray } => {
            let mut config = Config::load(&path)?;
            if no_tray {
                config.tray.enabled = false;
            }
            koe::daemon::run(config, &socket, Some(path)).await?;
        }
        Command::Enable => {
            let config = Config::load(&path)?;
            ensure!(
                config.engine.backend == config::Backend::Http || config.engine.model.is_file(),
                "there's no speech model yet: download one in Koe, or with aizome-koe model download"
            );
            service::set_enabled(&path, true)?;
            match service::ensure_shortcuts(&aizome_config::Settings::path(), true) {
                Ok(chords) if !chords.is_empty() => {
                    println!("Dictation's shortcut: {}", service::keycaps(&chords[0]).concat())
                }
                Ok(_) => println!("Dictation has no shortcut; give it one in Settings ▸ Keyboard"),
                Err(error) => eprintln!("aizome-koe: dictation has no shortcut: {error:#}"),
            }
            tokio::task::spawn_blocking(move || session.start()).await??;
            println!("Dictation is on");
        }
        Command::Disable => {
            // The service stops even when the choice can't be saved: a
            // configuration that can't be read is off at login already.
            let saved = service::set_enabled(&path, false);
            tokio::task::spawn_blocking(move || session.stop()).await??;
            saved?;
            println!("Dictation is off");
        }
        Command::Enabled => unreachable!("answered before the runtime starts"),
        Command::Packs { command } => packs(&Vocab::open(&path)?, command)?,
        Command::Words { command } => words(&Vocab::open(&path)?, command)?,
        Command::Examples { command } => examples(&Vocab::open(&path)?, command)?,
        Command::Status => control(&socket, Action::Status).await?,
        Command::Record { action: Action::Subscribe } => watch(&socket, false).await?,
        Command::Record { action } => record(session, action).await?,
        Command::Watch { waybar } => watch(&socket, waybar).await?,
        Command::Transcribe { input, model } => {
            let mut config = Config::load(&path)?;
            if let Some(model) = model {
                config.engine.model = model;
            }
            let samples = koe::audio::read_wav(&input, config.audio.max_seconds)?;
            let pipeline = koe::vocabulary::Store::user()?.load()?.pipeline(
                &config.engine.prompt,
                &config.replacements,
                config.output.append_space,
            )?;
            let engine = koe::engine::Recognizer::new(config.engine).with_prompt(pipeline.prompt);
            let text = engine.transcribe(samples, Arc::new(AtomicBool::new(false))).await?;
            println!("{}", pipeline.processor.apply(&text));
        }
        Command::Config { command } => match command {
            ConfigCommand::Show => print!("{}", toml::to_string_pretty(&Config::load(&path)?)?),
            ConfigCommand::Check => {
                Config::load(&path)?;
                println!("Configuration valid: {}", path.display());
            }
            ConfigCommand::Init => {
                use std::{io::Write, os::unix::fs::OpenOptionsExt};
                std::fs::create_dir_all(path.parent().context("configuration has no parent")?)?;
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)
                    .context("cannot create configuration (existing files are never overwritten)")?;
                file.write_all(toml::to_string_pretty(&Config::default())?.as_bytes())?;
                println!("{}", path.display());
            }
        },
        Command::Model { command } => match command {
            ModelCommand::List => {
                let dir = koe::models::directory()?;
                for model in koe::models::catalog().models {
                    println!(
                        "{:<9} {:>7}  {}",
                        model.name,
                        koe::models::size_text(model.bytes),
                        match koe::models::installed(&dir, &model) {
                            koe::models::Installed::Present => "present (use model verify to check)",
                            koe::models::Installed::Damaged => "incomplete or not the catalog's",
                            koe::models::Installed::Missing => "not installed",
                        }
                    );
                }
            }
            ModelCommand::Download { name, directory } => {
                let dir = directory.map(Ok).unwrap_or_else(koe::models::directory)?;
                println!("{}", koe::models::download(&name, &dir).await?.display());
            }
            ModelCommand::Verify { name, directory } => {
                let model = koe::models::find(&name)?;
                let dir = directory.map(Ok).unwrap_or_else(koe::models::directory)?;
                koe::models::verify(&dir.join(model.file_name()), &model)?;
                println!("{name}: verified");
            }
            ModelCommand::Remove { name, directory } => {
                let model = koe::models::find(&name)?;
                let dir = directory.map(Ok).unwrap_or_else(koe::models::directory)?;
                let config = Config::load(&path)?;
                ensure!(
                    !(config.enabled && config.engine.model == dir.join(model.file_name())),
                    "dictation uses {name}: turn it off, or choose another model, first"
                );
                koe::models::remove(&dir, &model)?;
                println!("{name}: removed");
            }
            ModelCommand::Unload => control(&socket, Action::Unload).await?,
        },
        Command::Doctor => {
            let config = Config::load(&path)?;
            let mut missing = false;
            println!("Configuration: {} (valid)", path.display());
            println!("Dictation: {}", if config.enabled { "on" } else { "off" });
            println!("Backend: {:?}; local compiled: {}", config.engine.backend, cfg!(feature = "local"));
            #[cfg(feature = "local")]
            println!(
                "CPU acceleration: {}",
                if cfg!(feature = "openblas") {
                    "OpenBLAS (runtime CPU selection)"
                } else {
                    "built-in Whisper kernels"
                }
            );
            println!(
                "GPU: requested {}; Vulkan compiled {}; CUDA compiled {}",
                config.engine.gpu,
                cfg!(feature = "vulkan"),
                cfg!(feature = "cuda")
            );
            if config.engine.gpu && koe::config::GPU_COMPILED {
                println!(
                    "GPU device: {}",
                    if koe::engine::gpu_available() { "found" } else { "none starts; the processor is used" }
                );
            }
            println!("System tray: {}", if config.tray.enabled { "enabled" } else { "disabled" });
            if config.engine.backend == config::Backend::Local {
                let ok = config.engine.model.is_file() && cfg!(feature = "local");
                println!(
                    "Local model: {} ({})",
                    config.engine.model.display(),
                    if ok { "present" } else { "missing model or backend" }
                );
                missing |= !ok;
            }
            let mut programs = vec!["pw-record"];
            if config.output.mode != config::OutputMode::None {
                programs.push("wl-copy");
            }
            if config.output.mode == config::OutputMode::Paste && !koe::desktop::uses_aizome(&config.output) {
                programs.push("wtype");
            }
            for program in programs {
                let found = std::env::var_os("PATH").is_some_and(|p| {
                    std::env::split_paths(&p).any(|dir| {
                        use std::os::unix::fs::PermissionsExt;
                        std::fs::metadata(dir.join(program))
                            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                    })
                });
                println!("{program}: {}", if found { "found" } else { "missing" });
                missing |= !found;
            }
            println!("Aizome IPC: {}", if koe::desktop::aizome_socket().is_some() { "available" } else { "unset" });
            let shortcuts = aizome_config::Settings::load()
                .map(|s| service::chords(&s.keys.bindings, service::TOGGLE))
                .unwrap_or_default();
            println!("Shortcut: {}", shortcuts.first().map_or("none".into(), |c| service::keycaps(c).concat()));
            if socket.as_os_str().is_empty() {
                println!("Runtime: XDG_RUNTIME_DIR is unset; run Koe in your desktop session");
                missing = true;
            } else {
                println!(
                    "Daemon socket: {} ({})",
                    socket.display(),
                    if service::running(&socket) { "running" } else { "not running" }
                );
            }
            ensure!(!missing, "resolve the missing dependencies above");
        }
    }
    Ok(())
}

/// Work dictation, starting its service if it's on but not running. With
/// dictation off, a shortcut to start it opens Koe, where it's turned on.
async fn record(session: Session, action: Action) -> Result<()> {
    if ipc::request(&session.socket, &Request::new(Action::Status)).await.is_err() {
        if !service::enabled(&session.config) {
            if matches!(action, Action::Toggle | Action::Start) {
                koe::desktop::open_window(window_args(&session, Page::Dictation)).await?;
            }
            bail!("dictation is off; turn it on in Koe");
        }
        let socket = session.socket.clone();
        tokio::task::spawn_blocking(move || session.start()).await??;
        return control(&socket, action).await;
    }
    control(&session.socket, action).await
}

/// Vocabulary commands use the store directly, with the window's lock and
/// revision checks, so scripts and agents work whether or not the daemon
/// runs. An open vocabulary window shows their changes within seconds.
struct Vocab {
    store: Store,
    config: Config,
}
impl Vocab {
    fn open(config: &Path) -> Result<Self> {
        Ok(Self { store: Store::user()?, config: Config::load(config)? })
    }
    fn dispatch(&self, command: V) -> Result<VocabularyReply> {
        self.store.dispatch(command, &self.config)
    }
    /// Change the current library and save it, starting again when another
    /// window or command saved first.
    fn edit<T>(&self, mut edit: impl FnMut(&mut Library) -> Result<T>) -> Result<(Library, T)> {
        let mut attempts = 0;
        loop {
            let mut library = self.store.load()?;
            let revision = library.revision;
            let result = edit(&mut library)?;
            match self.dispatch(V::SaveLibrary { library }) {
                Ok(reply) => return Ok((reply.library.context("library was not saved")?, result)),
                Err(_) if attempts < 3 && self.store.load()?.revision != revision => attempts += 1,
                Err(error) => return Err(error),
            }
        }
    }
    fn pack(&self, library: &Library, id: &str) -> Result<Value> {
        let pack = library.packs.iter().find(|p| p.id == id).context("pack no longer exists")?;
        let examples = self.store.examples(id, None)?;
        Ok(json!({
            "id": pack.id,
            "name": pack.name,
            "description": pack.description,
            "enabled": library.enabled.contains(&pack.id),
            "words": pack.words.iter().map(|w| word_json(w, &examples)).collect::<Vec<_>>(),
        }))
    }
    fn word(&self, library: &Library, pack: &str, id: &str) -> Result<Value> {
        let mut word = word_json(library.word(pack, id)?, &self.store.examples(pack, Some(id))?);
        word["pack"] = pack.into();
        Ok(word)
    }
}
fn word_json(word: &Word, examples: &[Example]) -> Value {
    json!({
        "id": word.id,
        "spelling": word.spelling,
        "aliases": word.aliases,
        "examples": examples.iter().filter(|e| e.word_id == word.id).count(),
    })
}
fn print(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}
fn pack_id(library: &Library, key: &str) -> Result<String> {
    if library.packs.iter().any(|p| p.id == key) {
        return Ok(key.into());
    }
    match library.packs.iter().filter(|p| same(&p.name, key)).collect::<Vec<_>>()[..] {
        [pack] => Ok(pack.id.clone()),
        [] => bail!("no pack {key}; see aizome-koe packs list"),
        _ => bail!("several packs are named {key}; use its ID"),
    }
}
fn pack_mut<'a>(library: &'a mut Library, key: &str) -> Result<&'a mut Pack> {
    let id = pack_id(library, key)?;
    Ok(library.packs.iter_mut().find(|p| p.id == id).unwrap())
}
fn word_id(pack: &Pack, key: &str) -> Result<String> {
    if pack.words.iter().any(|w| w.id == key) {
        return Ok(key.into());
    }
    match pack.words.iter().filter(|w| same(&w.spelling, key)).collect::<Vec<_>>()[..] {
        [word] => Ok(word.id.clone()),
        [] => bail!("{} has no word {key}; see aizome-koe packs show {}", pack.name, pack.id),
        _ => bail!("several words are spelled {key}; use its ID"),
    }
}

fn enable(library: &mut Library, pack: &str, on: bool) -> Result<String> {
    let id = pack_id(library, pack)?;
    if !on {
        library.enabled.retain(|p| p != &id);
    } else if !library.enabled.contains(&id) {
        library.enabled.push(id.clone());
    }
    Ok(id)
}

fn packs(vocab: &Vocab, command: PackCommand) -> Result<()> {
    let library = vocab.store.load()?;
    let (library, id) = match command {
        PackCommand::List => return print(&library),
        PackCommand::Show { pack } => {
            let id = pack_id(&library, &pack)?;
            (library, id)
        }
        PackCommand::Create { name, description, id, enable } => vocab.edit(|library| {
            let id = match &id {
                Some(id) => id.clone(),
                None => vocabulary::readable_id(&name, |id| library.packs.iter().any(|p| p.id == id))?,
            };
            ensure!(!library.packs.iter().any(|p| p.id == id), "pack ID {id} is already used");
            library.packs.push(Pack {
                id: id.clone(),
                name: name.trim().into(),
                description: description.trim().into(),
                words: vec![],
            });
            if enable {
                library.enabled.push(id.clone());
            }
            Ok(id)
        })?,
        PackCommand::Edit { pack, name, description } => {
            ensure!(name.is_some() || description.is_some(), "pass --name or --description");
            vocab.edit(|library| {
                let pack = pack_mut(library, &pack)?;
                if let Some(name) = &name {
                    pack.name = name.trim().into();
                }
                if let Some(description) = &description {
                    pack.description = description.trim().into();
                }
                Ok(pack.id.clone())
            })?
        }
        PackCommand::Remove { pack } => {
            let (_, id) = vocab.edit(|library| {
                let id = pack_id(library, &pack)?;
                library.packs.retain(|p| p.id != id);
                library.enabled.retain(|p| p != &id);
                Ok(id)
            })?;
            return print(&json!({ "removed": id }));
        }
        PackCommand::Import { path } => {
            let library = vocab
                .dispatch(V::Import { path, revision: library.revision })?
                .library
                .context("pack was not imported")?;
            let id = library.packs.last().context("pack was not imported")?.id.clone();
            (library, id)
        }
        PackCommand::Enable { pack } => vocab.edit(|library| enable(library, &pack, true))?,
        PackCommand::Disable { pack } => vocab.edit(|library| enable(library, &pack, false))?,
        PackCommand::Export { pack, path } => {
            let pack = pack_id(&library, &pack)?;
            return print(&vocab.dispatch(V::Export { pack, path })?);
        }
        PackCommand::ExportExamples { pack, path } => {
            let pack = pack_id(&library, &pack)?;
            return print(&vocab.dispatch(V::ExportExamples { pack, path })?);
        }
    };
    print(&vocab.pack(&library, &id)?)
}

fn words(vocab: &Vocab, command: WordCommand) -> Result<()> {
    let (library, (pack, id)) = match command {
        WordCommand::Add { pack, spelling, aliases, id } => vocab.edit(|library| {
            let pack = pack_mut(library, &pack)?;
            let spelling = spelling.trim();
            ensure!(
                !pack.words.iter().any(|w| same(&w.spelling, spelling)),
                "{} already has {spelling}; use aizome-koe words edit",
                pack.name
            );
            let id = match &id {
                Some(id) => id.clone(),
                None => vocabulary::readable_id(spelling, |id| pack.words.iter().any(|w| w.id == id))?,
            };
            pack.words.push(Word {
                id: id.clone(),
                spelling: spelling.into(),
                aliases: aliases.iter().map(|a| a.trim().into()).collect(),
            });
            Ok((pack.id.clone(), id))
        })?,
        WordCommand::Edit { pack, word, spelling, add, remove } => {
            ensure!(
                spelling.is_some() || !add.is_empty() || !remove.is_empty(),
                "pass --spelling, --add-alias or --remove-alias"
            );
            vocab.edit(|library| {
                let pack = pack_mut(library, &pack)?;
                let id = word_id(pack, &word)?;
                if let Some(spelling) = &spelling {
                    ensure!(
                        !pack.words.iter().any(|w| w.id != id && same(&w.spelling, spelling.trim())),
                        "{} already has {spelling}",
                        pack.name
                    );
                }
                let word = pack.words.iter_mut().find(|w| w.id == id).unwrap();
                if let Some(spelling) = &spelling {
                    word.spelling = spelling.trim().into();
                }
                for alias in &remove {
                    let count = word.aliases.len();
                    word.aliases.retain(|a| !same(a, alias.trim()));
                    ensure!(word.aliases.len() < count, "{} has no correction {alias}", word.spelling);
                }
                for alias in &add {
                    if !word.aliases.iter().any(|a| same(a, alias.trim())) {
                        word.aliases.push(alias.trim().into());
                    }
                }
                Ok((pack.id.clone(), id))
            })?
        }
        WordCommand::Remove { pack, word } => {
            let (_, (pack, id)) = vocab.edit(|library| {
                let pack = pack_mut(library, &pack)?;
                let id = word_id(pack, &word)?;
                pack.words.retain(|w| w.id != id);
                Ok((pack.id.clone(), id))
            })?;
            return print(&json!({ "removed": id, "pack": pack }));
        }
    };
    print(&vocab.word(&library, &pack, &id)?)
}

fn examples(vocab: &Vocab, command: ExampleCommand) -> Result<()> {
    match command {
        ExampleCommand::List { pack, word } => {
            let library = vocab.store.load()?;
            let packs = match &pack {
                Some(pack) => vec![pack_id(&library, pack)?],
                None => library.packs.iter().map(|p| p.id.clone()).collect(),
            };
            let word = match &word {
                Some(word) => Some(word_id(library.packs.iter().find(|p| p.id == packs[0]).unwrap(), word)?),
                None => None,
            };
            let mut examples = vec![];
            for pack in &packs {
                examples.extend(vocab.store.examples(pack, word.as_deref())?);
            }
            examples.sort_by_key(|e| std::cmp::Reverse(e.created_ms));
            let mut records = vec![];
            for example in examples {
                let audio = vocab.store.example_audio(&example.id)?;
                let mut record = serde_json::to_value(example)?;
                record["audio"] = json!(audio);
                records.push(record);
            }
            print(&records)
        }
        ExampleCommand::Delete { id } => {
            vocab.store.delete_example(&id).with_context(|| format!("cannot delete saved example {id}"))?;
            print(&json!({ "removed": id }))
        }
    }
}

async fn control(socket: &std::path::Path, action: Action) -> Result<()> {
    let response = ipc::request(socket, &Request::new(action)).await?;
    if action == Action::Last && response.ok {
        println!("{}", response.text.as_deref().unwrap_or_default());
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }
    ensure!(response.ok, "{}", response.error.unwrap_or_else(|| "request failed".into()));
    Ok(())
}
async fn watch(socket: &std::path::Path, waybar: bool) -> Result<()> {
    let mut stream = UnixStream::connect(socket).await.context("cannot connect to Koe; start the daemon first")?;
    ipc::write_json(&mut stream, &Request::new(Action::Subscribe)).await?;
    let mut reader = BufReader::new(stream);
    while let Some(line) = ipc::read_line(&mut reader).await? {
        let response: Response = serde_json::from_slice(&line)?;
        ensure!(response.ok, "subscription failed");
        if waybar {
            let phase = serde_json::to_value(response.status.phase)?.as_str().unwrap().to_owned();
            println!(
                "{}",
                serde_json::json!({"text":if phase == "idle" { "" } else { "Koe" },"class":phase,"alt":phase,"tooltip":format!("Koe: {phase} · {:.1}s", response.status.elapsed_ms as f64 / 1000.0)})
            );
        } else {
            println!("{}", serde_json::to_string(&response)?);
        }
    }
    Ok(())
}
