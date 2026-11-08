//! Everything Yukimi shows, read away from the interface thread in two
//! parts: what takes a few milliseconds (the configuration, generations,
//! sources), shown at once, and the store (its database, roots and what
//! they keep), which can take seconds and fills in when it is read.
//!
//! The same model as the GTK Yukimi's (Sekka's `apps/yukimi/yukimi/src/
//! model.rs`), read through [`Paths`] so another system's `/etc` can be
//! looked at, and with each generation's size for the Overview.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use yukimi_config::edit::{self, ListedPackage};
use yukimi_config::lock::{FlakeLock, Input};
use yukimi_config::packages::{self, Choices};
use yukimi_store::{Graph, Keep, PathId, Root, RootKind, StoreDb, StorePath, Tally, Usage, roots};
use yukimi_system::catalog::{self, Application};
use yukimi_system::channels::{self, Channel};
use yukimi_system::diff::{self, Diff};
use yukimi_system::generations::{self, Generation};
use yukimi_system::info::SystemInfo;
use yukimi_system::nix;
use yukimi_system::profile::{self, Element};
use yukimi_system::reclaim::{self, Reclaim};
use yukimi_system::setup::{Kind, Setup};

use crate::paths::Paths;

/// A package in the running system, as its store paths name it.
#[derive(Clone, Debug)]
pub struct SystemPackage {
    pub name: String,
    pub version: String,
    pub size: u64,
}

/// The store, divided by what keeps each part of it (NAR sizes). A part
/// shared by several keepers counts once, with the first of: the running
/// system (and the started one, and what running programs use), users'
/// profiles, projects, older generations.
#[derive(Clone, Debug, Default)]
pub struct Composition {
    /// The running (and started) system, and what programs running now use.
    pub system: u64,
    /// Kept only by older system generations: what deleting them all lets go.
    pub old_generations: u64,
    /// Kept by users' own packages.
    pub profiles: u64,
    /// Kept by build results and development shells.
    pub projects: u64,
    /// Kept by nothing: garbage collection would free it.
    pub garbage: u64,
    pub total: u64,
    pub paths: usize,
    pub garbage_paths: usize,
}

/// A package in a store, at one version or with its versions grouped, with
/// the size of all its paths.
#[derive(Clone, Debug)]
pub struct Heavy {
    pub name: String,
    /// Its versions, newest first: one, unless grouped by app.
    pub versions: Vec<String>,
    pub size: u64,
    /// How many builds the paths come from: two llvm 21.1.8s made
    /// differently are two copies.
    pub builds: usize,
}

/// What the configuration says is installed, as Yukimi can read it.
#[derive(Clone, Debug, Default)]
pub struct Configured {
    /// What `yukimi.nix` installs.
    pub yukimi: Choices,
    /// `environment.systemPackages` in the main configuration file.
    pub system: Vec<ListedPackage>,
    /// The current user's `packages` in the main configuration file.
    pub user: Vec<ListedPackage>,
    /// For each setting a system catalog installs through, the ids listed.
    pub settings: BTreeMap<String, Vec<String>>,
    /// For each of the system's own choices (its desktops, its login
    /// screen), what the configuration says now, or what the running
    /// system has where it says nothing.
    pub choices: BTreeMap<String, Vec<String>>,
}

/// The store as read from its database.
#[derive(Default)]
pub struct Store {
    pub graph: Graph,
    /// The links that keep store paths, as Yukimi finds them, to list.
    pub roots: Vec<Root>,
    pub composition: Composition,
    /// Which paths deleting generations lets go, from every root the
    /// garbage collector has.
    pub reclaim: Reclaim,
    /// Packages of the running system's `environment.systemPackages`.
    pub system_packages: Vec<SystemPackage>,
    /// The heaviest packages in the running system, one for each version.
    pub heaviest: Vec<Heavy>,
    /// The same with each app's versions grouped.
    pub heaviest_apps: Vec<Heavy>,
    /// How big each system generation is, oldest first: everything it
    /// needs (its closure), shared with the others or not. Not what
    /// deleting one gives back; that is [`Freeable`].
    pub generation_sizes: Vec<(u32, u64)>,
    /// What the newest generation changed from the one before.
    pub latest: Option<Diff>,
}

/// What deleting generations would give back on disk, once garbage is
/// collected: blocks, with hard-linked files counted once.
#[derive(Clone, Debug)]
pub struct Freeable {
    /// What collecting garbage gives back now: what nothing keeps.
    pub garbage: u64,
    /// For each generation that may be deleted, what deleting it alone
    /// gives back besides the garbage.
    pub generations: BTreeMap<u32, u64>,
    /// For deleting generations older than a number of days, as Clean Up
    /// does: the days, the generations that go, and what they give back
    /// besides the garbage.
    pub older: Vec<(u32, Vec<u32>, u64)>,
    /// What prices any set of generations deleted together.
    pricing: Pricing,
}

/// How [`Freeable`] prices generations deleted together. Generations next
/// to each other share most of what they have, so a set can give back far
/// more than its generations do one at a time.
#[derive(Clone)]
pub enum Pricing {
    /// The store that was measured (its graph and roots), its paths
    /// measured, and the garbage's, which goes in any case.
    Measured { store: Arc<Store>, usage: Usage, garbage: Tally },
    /// Sets of generations, each with what only that set keeps.
    Listed(Vec<(Vec<u32>, u64)>),
}

impl std::fmt::Debug for Pricing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pricing::Measured { usage, .. } => f.debug_struct("Measured").field("usage", usage).finish_non_exhaustive(),
            Pricing::Listed(sets) => f.debug_tuple("Listed").field(sets).finish(),
        }
    }
}

impl Pricing {
    /// Whether generation `number`'s link was among the roots priced.
    fn has(&self, number: u32) -> bool {
        match self {
            Pricing::Measured { store, .. } => store.reclaim.has(number),
            Pricing::Listed(_) => true,
        }
    }

    /// What deleting `numbers` together gives back besides the garbage.
    fn price(&self, numbers: &[u32]) -> u64 {
        match self {
            Pricing::Measured { store, usage, garbage } => {
                usage.freed_beyond(garbage, store.reclaim.only_kept_by(&store.graph, numbers))
            }
            Pricing::Listed(sets) => sets
                .iter()
                .filter(|(keepers, _)| keepers.iter().all(|k| numbers.contains(k)))
                .map(|(_, bytes)| bytes)
                .sum(),
        }
    }
}

/// The ages Clean Up offers, in days.
pub const CLEAN_UP_DAYS: [u32; 3] = [7, 30, 90];

impl Freeable {
    /// Measure what deleting each generation, and cleaning up, gives back.
    /// Reads the store's files: seconds, not milliseconds.
    pub fn measure(store: Arc<Store>, generations: &[Generation], now: i64) -> Freeable {
        let usage = Usage::measure(&store.graph, store.reclaim.candidates());
        Freeable::from_usage(store, usage, generations, now)
    }

    /// From the store's paths, measured. The store is kept to price sets
    /// of generations later: a store read since has paths of its own.
    pub fn from_usage(store: Arc<Store>, usage: Usage, generations: &[Generation], now: i64) -> Freeable {
        let base = usage.tally(store.reclaim.garbage());
        let garbage = usage.bytes(&base);
        Freeable::priced(garbage, Pricing::Measured { store, usage, garbage: base }, generations, now)
    }

    /// Each generation that may be deleted, alone, and Clean Up's sets, as
    /// `pricing` prices them.
    pub fn priced(garbage: u64, pricing: Pricing, generations: &[Generation], now: i64) -> Freeable {
        let each = generations
            .iter()
            .filter(|g| g.deletable() && pricing.has(g.number))
            .map(|g| (g.number, pricing.price(&[g.number])))
            .collect();
        let older = CLEAN_UP_DAYS
            .iter()
            .map(|&days| {
                let numbers = generations::older_than(generations, now - i64::from(days) * 86_400);
                let bytes = if numbers.is_empty() { 0 } else { pricing.price(&numbers) };
                (days, numbers, bytes)
            })
            .collect();
        Freeable { garbage, generations: each, older, pricing }
    }

    /// What deleting generation `number` gives back, once garbage is
    /// collected, if it may be deleted.
    pub fn generation(&self, number: u32) -> Option<u64> {
        self.generations.get(&number).copied()
    }

    /// What deleting `numbers` together gives back besides the garbage:
    /// what each has alone, and what only they share. Follows each one's
    /// closure, so not on the interface thread.
    pub fn together(&self, numbers: &[u32]) -> u64 {
        self.pricing.price(numbers)
    }

    /// Deleting generations older than `days`: which go, and what they give
    /// back besides the garbage.
    pub fn older(&self, days: u32) -> (&[u32], u64) {
        self.older.iter().find(|(d, _, _)| *d == days).map_or((&[], 0), |(_, numbers, bytes)| (numbers, *bytes))
    }
}

#[derive(Clone)]
pub struct Model {
    pub setup: Setup,
    pub info: SystemInfo,
    pub completion: yukimi_system::completion::Completion,
    pub generations: Vec<Generation>,
    /// Packages the current user installed for themselves.
    pub user_packages: Vec<Element>,
    pub configured: Configured,
    /// Sekka's evaluated application ids, including desktop defaults.
    /// Kept apart from the editable lists so adding or removing an app
    /// never writes these implicit choices into the configuration.
    pub installer_applications: BTreeSet<String>,
    /// The current user's name, whose `packages` are read.
    pub user: String,
    /// The system accepts unfree packages.
    pub allow_unfree: bool,
    pub catalog: Vec<Application>,
    /// A flake's inputs, from `flake.lock`.
    pub inputs: Vec<Input>,
    pub update_history: crate::update_history::History,
    /// Where `flake.nix` says Sekka comes from, when it does.
    pub sekka_url: Option<String>,
    /// A channel system's channels.
    pub channels: Vec<Channel>,
    /// Store path of the system's Nixpkgs, for searching it.
    pub nixpkgs: Option<String>,
    /// The store, once read.
    pub store: Option<Arc<Store>>,
    /// What deleting generations would give back, once measured.
    pub freeable: Option<Arc<Freeable>>,
    /// Where the system was read from.
    pub paths: Paths,
    /// What could not be read, to say so instead of failing.
    pub problems: Vec<String>,
}

impl Default for Model {
    fn default() -> Model {
        Model {
            setup: Setup::detect_in(Path::new("/nonexistent"), Default::default(), ""),
            info: SystemInfo::default(),
            completion: Default::default(),
            generations: Vec::new(),
            user_packages: Vec::new(),
            configured: Configured::default(),
            installer_applications: BTreeSet::new(),
            user: String::new(),
            allow_unfree: false,
            catalog: Vec::new(),
            inputs: Vec::new(),
            update_history: Default::default(),
            sekka_url: None,
            channels: Vec::new(),
            nixpkgs: None,
            store: None,
            freeable: None,
            problems: Vec::new(),
            paths: Paths::default(),
        }
    }
}

impl Model {
    /// Everything but the store, loaded on a worker. Completion also inspects
    /// the desktop over IPC; a stalled compositor can take several seconds
    /// to time out, so this must never run on the interface thread.
    pub fn quick(paths: &Paths) -> Model {
        let mut setup = Setup::detect_in(&paths.etc_nixos(), paths.facts(), &yukimi_system::info::hostname());
        // Who logs in by themselves may be anyone with an account now.
        setup.facts.offer_people(&paths.people());
        let mut model = Model {
            info: SystemInfo::read(),
            completion: yukimi_system::completion::read(),
            generations: generations::system(),
            user_packages: profile::user_elements(),
            user: std::env::var("USER").unwrap_or_default(),
            catalog: catalog::for_system(&setup.facts),
            nixpkgs: setup.facts.nixpkgs.clone(),
            setup,
            paths: paths.clone(),
            ..Model::default()
        };
        model.read_configuration();
        model.read_choices();
        match model.setup.kind {
            Kind::Flake => model.read_lock(),
            Kind::Channels => model.channels = channels::root(),
        }
        if model.nixpkgs.is_none() && model.setup.kind == Kind::Channels {
            model.nixpkgs = std::fs::canonicalize(Path::new(channels::ROOT_CHANNELS).join("nixos"))
                .ok()
                .map(|p| p.to_string_lossy().into_owned());
        }
        model
    }

    fn read_configuration(&mut self) {
        self.read_installer_applications();
        let setup = &self.setup;
        if let Some(file) = setup.packages_file()
            && let Ok(text) = std::fs::read_to_string(&file)
        {
            match packages::read(&text) {
                Ok(choices) => self.configured.yukimi = choices,
                Err(e) => self.problems.push(format!("{}: {e}", file.display())),
            }
        }
        let Some(main) = &setup.main else {
            return;
        };
        let text = match std::fs::read_to_string(main) {
            Ok(text) => text,
            Err(e) => {
                self.problems.push(format!("{} could not be read: {e}", main.display()));
                return;
            }
        };
        self.configured.system =
            edit::package_list(&text, &["environment", "systemPackages"]).ok().flatten().unwrap_or_default();
        if !self.user.is_empty() {
            self.configured.user = edit::package_list(&text, &["users", "users", &self.user, "packages"])
                .ok()
                .flatten()
                .unwrap_or_default();
        }
        for source in &setup.facts.catalogs {
            let Some(setting) = &source.setting else { continue };
            let path: Vec<&str> = setting.split('.').collect();
            match edit::string_list(&text, &path) {
                Ok(list) => {
                    self.configured.settings.insert(setting.clone(), list.unwrap_or_default());
                }
                Err(e) => self.problems.push(format!("{setting}: {e}")),
            }
        }
        self.allow_unfree = setup.facts.allow_unfree.unwrap_or_else(|| {
            edit::bool_value(&text, &["nixpkgs", "config", "allowUnfree"]).ok().flatten().unwrap_or(false)
        });
    }

    /// What the configuration chooses of the system's own choices.
    fn read_choices(&mut self) {
        let text = self.setup.main.as_ref().and_then(|main| std::fs::read_to_string(main).ok());
        for choice in &self.setup.facts.choices {
            let path: Vec<&str> = choice.setting.split('.').collect();
            let written = text.as_deref().and_then(|text| {
                if choice.many {
                    edit::string_list(text, &path).ok().flatten()
                } else if choice.boolean {
                    edit::bool_value(text, &path).ok().flatten().map(|b| vec![b.to_string()])
                } else {
                    edit::string_value(text, &path).ok().flatten().map(|v| vec![v])
                }
            });
            self.configured.choices.insert(choice.setting.clone(), written.unwrap_or_else(|| choice.value.values()));
        }
    }

    /// Whether Sekka updates early: from its main branch, as soon as
    /// changes are merged, rather than from the nightly.
    pub fn early(&self) -> bool {
        self.sekka_url.as_deref().is_some_and(yukimi_config::sekka::early)
    }

    /// Sekka already records its evaluated choices at build time. Read
    /// that small manifest once with the rest of the background load,
    /// without evaluating Nix or waiting for the store database.
    fn read_installer_applications(&mut self) {
        self.installer_applications.clear();
        if !self.setup.facts.catalogs.iter().any(|source| source.setting.as_deref() == Some("calamares.applications")) {
            return;
        }
        #[derive(serde::Deserialize)]
        struct Manifest {
            selected: BTreeSet<String>,
        }
        let file = self.paths.etc.join("installer-applications.json");
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            // Older and non-Sekka systems keep the existing detection.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.problems.push(format!("{} could not be read: {e}", file.display()));
                return;
            }
        };
        match serde_json::from_str::<Manifest>(&text) {
            Ok(manifest) => self.installer_applications = manifest.selected,
            Err(e) => self.problems.push(format!("{}: {e}", file.display())),
        }
    }

    fn read_lock(&mut self) {
        if !self.paths.elsewhere {
            self.update_history =
                crate::update_history::History::load(Path::new(yukimi_system::SAVED_CONFIGURATIONS), &self.generations);
        }
        if let Ok(text) = std::fs::read_to_string(self.setup.dir.join("flake.nix")) {
            self.sekka_url = edit::string_value(&text, &["inputs", "sekka", "url"]).ok().flatten();
        }
        match std::fs::read_to_string(self.setup.dir.join("flake.lock")).map(|t| FlakeLock::parse(&t)) {
            Ok(Ok(lock)) => self.inputs = lock.inputs(),
            Ok(Err(e)) => self.problems.push(e.to_string()),
            Err(_) => {}
        }
    }

    /// This model with the store read.
    pub fn with_store(&self, store: Arc<Store>, problems: Vec<String>) -> Model {
        let mut model = Model { store: Some(store), ..self.clone() };
        model.problems.extend(problems);
        model
    }

    pub fn current_generation(&self) -> Option<&Generation> {
        // Several generation links can name the running build. Prefer the
        // selected one, as History does, then the newest active alias.
        self.generations.iter().filter(|g| g.active).max_by_key(|g| (g.current, g.number))
    }

    /// Attributes the current user installed for themselves.
    pub fn user_attrs(&self) -> BTreeSet<&str> {
        self.user_packages.iter().filter_map(|e| e.package_attr()).collect()
    }

    /// Why Yukimi can't change what the system installs, when it can't: it
    /// is looking at another system's configuration, or it didn't find the
    /// configuration file to add its own to.
    pub fn cannot_change_system(&self) -> Option<String> {
        if self.paths.elsewhere {
            return Some(format!(
                "Yukimi is looking at the configuration in {}, not this computer's own, so it changes nothing",
                self.paths.etc_nixos().display()
            ));
        }
        self.setup.main.is_none().then(|| {
            format!(
                "Yukimi couldn't tell which file in {} is this computer's configuration, so it can't add to it",
                self.setup.dir.display()
            )
        })
    }

    /// The flake reference packages are installed from for one user: the
    /// system's own Nixpkgs, already in the store, so that what one person
    /// installs matches the system and nothing more is downloaded.
    pub fn nixpkgs_ref(&self) -> String {
        if let Some(path) = &self.nixpkgs {
            return format!("path:{path}");
        }
        let locked = self.inputs.iter().find(|i| i.name == "nixpkgs").and_then(|i| i.locked.as_ref());
        match locked {
            Some(l) if l.kind == "github" => match (&l.owner, &l.repo, &l.rev) {
                (Some(owner), Some(repo), Some(rev)) => format!("github:{owner}/{repo}/{rev}"),
                _ => "nixpkgs".to_owned(),
            },
            Some(l) => l.url.clone().unwrap_or_else(|| "nixpkgs".to_owned()),
            None => "nixpkgs".to_owned(),
        }
    }

    /// The names of packages the running system has, once the store is read.
    pub fn system_names(&self) -> BTreeSet<&str> {
        self.store.as_ref().map(|s| s.system_packages.iter().map(|p| p.name.as_str()).collect()).unwrap_or_default()
    }

    /// Whether a catalog application is installed: listed where it is
    /// installed through, supplied by Sekka's evaluated application
    /// selection, or (for Yukimi's own catalog) all its packages in the
    /// running system however they got there.
    pub fn app_installed(&self, app: &Application, system: &BTreeSet<&str>) -> bool {
        if let Some(setting) = &app.setting {
            return self.configured.settings.get(setting).is_some_and(|ids| ids.contains(&app.id))
                || (setting == "calamares.applications" && self.installer_applications.contains(&app.id));
        }
        let mine = &self.configured.yukimi;
        if let Some(program) = &app.program {
            return mine.programs.contains(program) || system.contains(program.as_str());
        }
        !app.packages.is_empty()
            && app
                .packages
                .iter()
                .all(|p| mine.packages.contains(p) || system.contains(p.rsplit('.').next().unwrap_or(p)))
    }
}

impl Store {
    /// Read the store database and work out what keeps what. Takes a few
    /// seconds on a large store.
    pub fn load(generations: &[Generation]) -> (Store, Vec<String>) {
        let mut problems = Vec::new();
        let mut store = Store { roots: roots::scan(), ..Store::default() };
        match StoreDb::open_default().and_then(|db| db.graph()) {
            Ok(graph) => store.graph = graph,
            Err(e) => problems.push(format!("The store database could not be read: {e}")),
        }
        // Every root the garbage collector has, as Nix lists them: with what
        // running programs use, even other people's. Without Nix to ask, the
        // links Yukimi can read.
        let (mut gc_roots, _) = reclaim::system_roots();
        if !gc_roots.iter().any(|r| r.kind == RootKind::CurrentSystem)
            && let Some(target) =
                std::fs::read_link("/run/current-system").ok().and_then(|p| StorePath::parse(p.to_str()?))
        {
            gc_roots.push(Root { link: "/run/current-system".into(), target, kind: RootKind::CurrentSystem });
        }
        let keep = nix::keep();
        store.composition = composition(&store.graph, &gc_roots, keep);
        store.reclaim = Reclaim::new(&store.graph, &gc_roots, keep);
        store.system_packages = system_packages(&store.graph);
        (store.heaviest, store.heaviest_apps) = heaviest(&store.graph, 24);
        store.generation_sizes = generations
            .iter()
            .map(|g| (g.number, store.closure_of(&g.target).iter().map(|&id| store.graph.info(id).nar_size).sum()))
            .collect();
        if let [.., before, after] = generations {
            let (before, after) = (store.closure_of(&before.target), store.closure_of(&after.target));
            store.latest = Some(diff::diff(&store.graph, &before, &after));
        }
        (store, problems)
    }

    /// The closure of a generation's build.
    pub fn closure_of(&self, target: &StorePath) -> Vec<PathId> {
        self.graph.lookup(target.as_str()).map(|id| self.graph.closure([id])).unwrap_or_default()
    }
}

fn ids(graph: &Graph, roots: &[&Root]) -> Vec<PathId> {
    roots.iter().filter_map(|r| graph.lookup(r.target.as_str())).collect()
}

/// What keeps each part of the store. A part counts with the first that
/// keeps it, in this order, so that "older generations" is what only they
/// keep: what deleting them all would let go.
fn composition(graph: &Graph, roots: &[Root], keep: Keep) -> Composition {
    let mut owner: Vec<u8> = vec![0; graph.len()];
    let of = |kind: fn(&RootKind) -> bool| -> Vec<&Root> { roots.iter().filter(|r| kind(&r.kind)).collect() };
    let groups: [(u8, Vec<&Root>); 4] = [
        (1, of(|k| matches!(k, RootKind::CurrentSystem | RootKind::BootedSystem | RootKind::Running))),
        (3, of(|k| matches!(k, RootKind::Profile { .. }))),
        (4, of(|k| matches!(k, RootKind::BuildResult | RootKind::DevShell | RootKind::Other))),
        (2, of(|k| matches!(k, RootKind::SystemGeneration(_)))),
    ];
    for (tag, group) in &groups {
        for id in graph.kept(ids(graph, group), keep) {
            if owner[id.index()] == 0 {
                owner[id.index()] = *tag;
            }
        }
    }
    let mut c = Composition { paths: graph.len(), ..Default::default() };
    for id in graph.ids() {
        let size = graph.info(id).nar_size;
        c.total += size;
        match owner[id.index()] {
            1 => c.system += size,
            2 => c.old_generations += size,
            3 => c.profiles += size,
            4 => c.projects += size,
            _ => {
                c.garbage += size;
                c.garbage_paths += 1;
            }
        }
    }
    c
}

/// The packages in the running system's `environment.systemPackages`: what
/// its `sw` profile refers to.
fn system_packages(graph: &Graph) -> Vec<SystemPackage> {
    let Some(sw) = std::fs::read_link("/run/current-system/sw").ok().and_then(|p| graph.lookup(p.to_str()?)) else {
        return Vec::new();
    };
    let mut by_name: BTreeMap<String, SystemPackage> = BTreeMap::new();
    for &id in graph.references(sw) {
        let Some(path) = graph.store_path(id) else { continue };
        let entry = by_name.entry(path.name().to_owned()).or_insert_with(|| SystemPackage {
            name: path.name().to_owned(),
            version: path.version().to_owned(),
            size: 0,
        });
        entry.size += graph.info(id).nar_size;
    }
    by_name.into_values().collect()
}

/// The heaviest packages in the running system's closure: one for each
/// version, and with each app's versions grouped.
fn heaviest(graph: &Graph, count: usize) -> (Vec<Heavy>, Vec<Heavy>) {
    let Some(current) = std::fs::read_link("/run/current-system").ok().and_then(|p| graph.lookup(p.to_str()?)) else {
        return Default::default();
    };
    let versions = by_version(graph, current);
    (heaviest_of(versions.clone(), count), heaviest_of(by_app(versions), count))
}

/// The `count` largest.
fn heaviest_of(mut list: Vec<Heavy>, count: usize) -> Vec<Heavy> {
    list.sort_by_key(|h| std::cmp::Reverse(h.size));
    list.truncate(count);
    list
}

/// The packages in `top`'s closure, one for each name and version: a
/// build's outputs (`lib32`, `firmware`, `jre`, …) together, named as the
/// derivation that made them when the store records it, and other versions
/// (openjdk 25, 21 and 17) apart.
fn by_version(graph: &Graph, top: PathId) -> Vec<Heavy> {
    let mut by_version: HashMap<(String, String), (Heavy, BTreeSet<String>)> = HashMap::new();
    for id in graph.closure([top]) {
        let Some(path) = graph.store_path(id) else { continue };
        if path.is_derivation() {
            continue;
        }
        let info = graph.info(id);
        // openjdk-8u504-b01-jre was made by openjdk-8u504-b01.drv.
        let made_by = info.deriver.as_deref().and_then(StorePath::parse);
        let (name, version) = match &made_by {
            Some(drv) => {
                let full = drv.full_name();
                yukimi_store::path::split_name(full.strip_suffix(".drv").unwrap_or(full))
            }
            None => (path.name(), path.version()),
        };
        let (heavy, builds) = by_version.entry((name.to_owned(), version.to_owned())).or_insert_with(|| {
            let heavy = Heavy { name: name.to_owned(), versions: vec![version.to_owned()], size: 0, builds: 0 };
            (heavy, BTreeSet::new())
        });
        heavy.size += info.nar_size;
        builds.insert(made_by.map_or_else(|| path.as_str().to_owned(), |drv| drv.as_str().to_owned()));
    }
    by_version.into_values().map(|(heavy, builds)| Heavy { builds: builds.len(), ..heavy }).collect()
}

/// Each app's versions grouped: their sizes and builds added up, the
/// versions newest first.
fn by_app(versions: Vec<Heavy>) -> Vec<Heavy> {
    let mut by_name: HashMap<String, Heavy> = HashMap::new();
    for heavy in versions {
        match by_name.get_mut(&heavy.name) {
            Some(app) => {
                app.size += heavy.size;
                app.builds += heavy.builds;
                app.versions.extend(heavy.versions);
            }
            None => {
                by_name.insert(heavy.name.clone(), heavy);
            }
        }
    }
    by_name
        .into_values()
        .map(|mut app| {
            app.versions.sort_by(|a, b| yukimi_store::compare_versions(b, a));
            app
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use yukimi_system::setup::CatalogSource;

    struct System {
        model: Model,
    }

    impl System {
        fn new(name: &str) -> Self {
            let etc = std::env::temp_dir().join(format!("yukimi-installed-{name}-{}", std::process::id()));
            std::fs::create_dir_all(etc.join("nixos")).unwrap();
            let main = etc.join("nixos/configuration.nix");
            std::fs::write(&main, r#"{ calamares.applications = [ "firefox" ]; }"#).unwrap();
            let mut model = Model { paths: Paths { etc, elsewhere: true }, ..Model::default() };
            model.setup.main = Some(main);
            model.setup.facts.catalogs.push(CatalogSource {
                file: String::new(),
                setting: Some("calamares.applications".into()),
                title: None,
            });
            Self { model }
        }

        fn manifest(&self, text: &str) {
            std::fs::write(self.model.paths.etc.join("installer-applications.json"), text).unwrap();
        }
    }

    impl Drop for System {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.model.paths.etc);
        }
    }

    fn app(id: &str, setting: Option<&str>) -> Application {
        Application {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            category: "Terminals".into(),
            packages: vec![id.into()],
            program: None,
            needs: None,
            unfree: false,
            requires: Vec::new(),
            unavailable: None,
            setting: setting.map(str::to_owned),
        }
    }

    #[test]
    fn desktop_defaults_are_installed_before_the_store_loads_without_becoming_editable_choices() {
        let mut system = System::new("defaults");
        system.manifest(r#"{"selected":["firefox","fish","fish"],"packages":[{"name":"fish-4.9.3"}]}"#);
        system.model.read_configuration();
        let model = &system.model;
        assert!(model.store.is_none());
        assert!(model.app_installed(&app("fish", Some("calamares.applications")), &BTreeSet::new()));
        assert_eq!(model.configured.settings["calamares.applications"], ["firefox"]);
        assert_eq!(model.installer_applications.len(), 2);
        assert!(!model.app_installed(&app("kitty", Some("calamares.applications")), &BTreeSet::new()));
        assert!(model.problems.is_empty());

        // The store's later arrival preserves the already-known defaults.
        let model = model.with_store(Arc::new(Store::default()), Vec::new());
        assert!(model.app_installed(&app("fish", Some("calamares.applications")), &model.system_names()));

        // A new generation can stop supplying a previously implicit app.
        system.manifest(r#"{"selected":["firefox"]}"#);
        system.model.read_configuration();
        assert!(!system.model.app_installed(&app("fish", Some("calamares.applications")), &BTreeSet::new()));
    }

    #[test]
    fn missing_or_invalid_manifests_preserve_explicit_choices() {
        let mut system = System::new("compatibility");
        // Missing on older systems: normal, and never borrow the host's manifest.
        system.model.read_configuration();
        assert!(system.model.installer_applications.is_empty());
        assert!(system.model.problems.is_empty());
        for text in ["{", "{}", r#"{"selected":"fish"}"#, r#"{"selected":[null]}"#] {
            system.manifest(text);
            system.model.problems.clear();
            system.model.read_configuration();
            assert!(system.model.installer_applications.is_empty());
            assert_eq!(system.model.problems.len(), 1);
            assert!(system.model.app_installed(&app("firefox", Some("calamares.applications")), &BTreeSet::new()));
            assert!(!system.model.app_installed(&app("fish", Some("calamares.applications")), &BTreeSet::new()));
        }
        system.manifest(r#"{"selected":[]}"#);
        system.model.problems.clear();
        system.model.read_configuration();
        assert!(system.model.problems.is_empty());
        assert!(system.model.app_installed(&app("firefox", Some("calamares.applications")), &BTreeSet::new()));
    }

    #[test]
    fn manifest_is_scoped_to_sekkas_catalog_and_does_not_require_editable_configuration() {
        let mut system = System::new("scope");
        system.manifest(r#"{"selected":["fish"]}"#);
        system.model.setup.main = None;
        system.model.read_configuration();
        let fish = app("fish", Some("calamares.applications"));
        assert!(system.model.app_installed(&fish, &BTreeSet::new()));
        assert!(!system.model.app_installed(&app("fish", Some("other.applications")), &BTreeSet::from(["fish"])));
        assert!(!system.model.app_installed(&app("fish", None), &BTreeSet::new()));
        assert!(system.model.app_installed(&app("fish", None), &BTreeSet::from(["fish"])));

        // Non-Sekka catalogs should not read this file at all.
        system.model.setup.facts.catalogs[0].setting = Some("other.applications".into());
        system.manifest("not JSON");
        system.model.read_configuration();
        assert!(system.model.installer_applications.is_empty());
        assert!(system.model.problems.is_empty());
    }

    /// A store of three generations, 3 running: 0 base, 1-3 systems, 4 only
    /// 1 has, 5 that 1 and a user's profile share, 6 garbage, 7 only 1 and
    /// 2 have.
    fn small_store() -> (Store, Vec<Generation>) {
        use yukimi_store::db::PathInfo;
        let path = |n: usize, name: &str| crate::fixture::store_path(n, name);
        let names = ["base", "system-1", "system-2", "system-3", "only-1", "shared", "garbage", "only-1-2"];
        let sizes = [1000, 1, 1, 1, 20, 40, 70, 30];
        let nodes: Vec<PathInfo> = names
            .iter()
            .zip(sizes)
            .enumerate()
            .map(|(i, (name, size))| PathInfo { path: path(i, name), nar_size: size, registered: 0, deriver: None })
            .collect();
        let graph = Graph::from_parts(nodes, &[(1, 0), (2, 0), (3, 0), (1, 4), (1, 5), (1, 7), (2, 7)]);
        let root = |link: &str, i: usize| {
            let link = std::path::PathBuf::from(link);
            Root { kind: roots::classify(&link), target: StorePath::parse(&path(i, names[i])).unwrap(), link }
        };
        let gc_roots = vec![
            root("/nix/var/nix/profiles/system-1-link", 1),
            root("/nix/var/nix/profiles/system-2-link", 2),
            root("/nix/var/nix/profiles/system-3-link", 3),
            root("/run/current-system", 3),
            root("/home/ref/.local/state/nix/profiles/profile-1-link", 5),
        ];
        let generations: Vec<Generation> = (1..=3)
            .map(|n| Generation {
                number: n,
                link: format!("/nix/var/nix/profiles/system-{n}-link").into(),
                target: StorePath::parse(&path(n as usize, names[n as usize])).unwrap(),
                created: i64::from(n) * 86_400,
                nixos_version: None,
                kernel_version: None,
                current: n == 3,
                active: n == 3,
                booted: n == 3,
            })
            .collect();
        let store = Store {
            composition: composition(&graph, &gc_roots, Keep::default()),
            reclaim: Reclaim::new(&graph, &gc_roots, Keep::default()),
            graph,
            ..Store::default()
        };
        (store, generations)
    }

    #[test]
    fn older_generations_are_what_only_they_keep() {
        let (store, _) = small_store();
        let c = &store.composition;
        // The profile's path counts as the user's, not as generation 1's.
        assert_eq!(c.profiles, 40);
        assert_eq!(c.old_generations, 1 + 1 + 20 + 30);
        assert_eq!(c.system, 1000 + 1);
        assert_eq!((c.garbage, c.garbage_paths), (70, 1));
    }

    #[test]
    fn what_deleting_each_generation_frees_is_measured_on_disk() {
        let (store, generations) = small_store();
        let dir = std::env::temp_dir().join(format!("yukimi-freeable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Each candidate on disk: a file a hundred times its NAR size.
        let paths: Vec<(PathId, std::path::PathBuf)> = store
            .reclaim
            .candidates()
            .into_iter()
            .map(|id| {
                let file = dir.join(id.index().to_string());
                std::fs::write(&file, vec![0u8; 100 * store.graph.info(id).nar_size as usize]).unwrap();
                (id, file)
            })
            .collect();
        let usage = Usage::measure_paths(paths, false);
        let size = |i: usize| usage.freed([PathId::from_index(i)]);
        let freeable = Freeable::from_usage(Arc::new(store), usage.clone(), &generations, 10 * 86_400);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(freeable.garbage, size(6));
        // 1: its build and what only it has, not the profile's share, nor
        // what it shares with 2.
        assert_eq!(freeable.generation(1), Some(size(1) + size(4)));
        assert_eq!(freeable.generation(2), Some(size(2)));
        // Together, what they share too.
        assert_eq!(freeable.together(&[1, 2]), size(1) + size(4) + size(2) + size(7));
        assert_eq!(freeable.together(&[2]), size(2));
        // The running one isn't offered.
        assert_eq!(freeable.generation(3), None);
        // A week ago, 2 was in use, so only 1 goes.
        assert_eq!(freeable.older(7), (&[1u32][..], size(1) + size(4)));
        assert_eq!(freeable.older(30), (&[][..], 0));
    }

    #[test]
    fn the_heaviest_are_by_version_with_a_builds_outputs_together() {
        use yukimi_store::db::PathInfo;
        let path = |n: usize, name: &str| crate::fixture::store_path(n, name);
        // Each path's name, NAR size and the derivation that made it.
        let paths: [(&str, u64, Option<&str>); 9] = [
            ("nixos-system-a-1", 1, None),
            ("openjdk-25.0.5.0.3+1", 688, Some("openjdk-25.0.5.0.3+1.drv")),
            ("openjdk-8u504-b01", 94, Some("openjdk-8u504-b01.drv")),
            ("openjdk-8u504-b01-jre", 109, Some("openjdk-8u504-b01.drv")),
            ("llvm-21.1.8-lib", 577, Some("llvm-21.1.8.drv")),
            ("llvm-21.1.8-lib", 564, Some("llvm-21.1.8.drv")),
            ("nvidia-x11-615.78.08-firmware", 117, Some("nvidia-x11-615.78.08.drv")),
            ("nvidia-x11-615.78.08", 958, Some("nvidia-x11-615.78.08.drv")),
            ("zstd-1.5.7-bin", 2, None),
        ];
        // The two llvms were made by different derivations; the others'
        // outputs share one.
        let drv = |i: usize, name: &str| path(if i == 5 { 105 } else { 100 + name.len() }, name);
        let nodes: Vec<PathInfo> = paths
            .iter()
            .enumerate()
            .map(|(i, &(name, size, made_by))| PathInfo {
                path: path(i, name),
                nar_size: size,
                registered: 0,
                deriver: made_by.map(|d| drv(i, d)),
            })
            .collect();
        let edges: Vec<(usize, usize)> = (1..paths.len()).map(|i| (0, i)).collect();
        let graph = Graph::from_parts(nodes, &edges);
        let versions = by_version(&graph, PathId::from_index(0));
        let heaviest = heaviest_of(versions.clone(), 10);
        let row = |name: &str, version: &str| {
            let h = heaviest.iter().find(|h| h.name == name && h.versions == [version]);
            h.map(|h| (h.size, h.builds)).unwrap_or_else(|| panic!("{name} {version} in {heaviest:?}"))
        };
        assert_eq!(row("llvm", "21.1.8"), (577 + 564, 2));
        assert_eq!(row("nvidia-x11", "615.78.08"), (117 + 958, 1));
        assert_eq!(row("openjdk", "25.0.5.0.3+1"), (688, 1));
        assert_eq!(row("openjdk", "8u504-b01"), (94 + 109, 1));
        // Without a derivation, the path's own name, its output taken off.
        assert_eq!(row("zstd", "1.5.7"), (2, 1));
        assert_eq!(heaviest[0].name, "llvm");
        assert_eq!(heaviest.len(), 6);
        // Grouped by app, openjdk's versions together, newest first.
        let apps = heaviest_of(by_app(versions), 10);
        let openjdk = apps.iter().find(|h| h.name == "openjdk").expect("openjdk");
        assert_eq!(openjdk.versions, ["25.0.5.0.3+1", "8u504-b01"]);
        assert_eq!((openjdk.size, openjdk.builds), (688 + 94 + 109, 2));
        assert_eq!(apps.len(), 5);
        assert_eq!(apps[0].name, "llvm");
    }

    #[test]
    #[ignore = "prints manifest loading and cached detection timings; run with --ignored --nocapture"]
    fn catalog_detection_cost() {
        use std::hint::black_box;
        use std::time::Instant;

        let mut system = System::new("timing");
        // Include the package data the reader skips, at about the size of
        // a real installed system's manifest.
        let manifest = serde_json::json!({
            "selected": ["firefox", "fish"],
            "packages": vec![serde_json::json!({
                "name": "fish-4.9.3",
                "path": "/nix/store/x6qcc23lrc64g0vjp9pfsa9scpy1mnpk-fish-4.9.3",
                "version": "4.9.3"
            }); 60]
        })
        .to_string();
        system.manifest(&manifest);
        let start = Instant::now();
        for _ in 0..1_000 {
            system.model.read_installer_applications();
            black_box(&system.model.installer_applications);
        }
        eprintln!("{}-byte manifest: {:.1} µs/read", manifest.len(), start.elapsed().as_secs_f64() * 1e3);
        let fish = app("fish", Some("calamares.applications"));
        let empty = BTreeSet::new();
        let start = Instant::now();
        for _ in 0..1_000_000 {
            assert!(black_box(&system.model).app_installed(black_box(&fish), black_box(&empty)));
        }
        eprintln!("cached detection: {:.1} ns/lookup", start.elapsed().as_secs_f64() * 1e3);
    }
}
