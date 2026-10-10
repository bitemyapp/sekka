// SPDX-License-Identifier: MIT OR Apache-2.0
//! Everything Yukimi shows, read away from the interface thread in two
//! parts: what takes a few milliseconds (the configuration, generations,
//! sources), shown at once, and the store (its database, roots and what
//! they keep), which can take seconds and fills in when it is read.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use yukimi_config::edit::{self, ListedPackage};
use yukimi_config::lock::{FlakeLock, Input};
use yukimi_config::packages::{self, Choices};
use yukimi_store::{Graph, PathId, Root, RootKind, StoreDb, StorePath, roots};
use yukimi_system::catalog::{self, Application};
use yukimi_system::channels::{self, Channel};
use yukimi_system::diff::{self, Diff};
use yukimi_system::generations::{self, Generation};
use yukimi_system::info::SystemInfo;
use yukimi_system::nix;
use yukimi_system::profile::{self, Element};
use yukimi_system::setup::{Kind, Setup};

/// A package in the running system, as its store paths name it.
#[derive(Clone, Debug)]
pub struct SystemPackage {
    pub name: String,
    pub version: String,
    pub size: u64,
}

/// The store, divided by what keeps each part of it.
#[derive(Clone, Debug, Default)]
pub struct Composition {
    /// The running (and started) system.
    pub system: u64,
    /// Kept only by older system generations.
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

/// A package in a store, by name, with the size of all its paths.
#[derive(Clone, Debug)]
pub struct Heavy {
    pub name: String,
    pub version: String,
    pub size: u64,
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
}

/// The store as read from its database.
#[derive(Default)]
pub struct Store {
    pub graph: Graph,
    pub roots: Vec<Root>,
    pub composition: Composition,
    /// Packages of the running system's `environment.systemPackages`.
    pub system_packages: Vec<SystemPackage>,
    /// The heaviest packages in the running system.
    pub heaviest: Vec<Heavy>,
    /// What the newest generation changed from the one before.
    pub latest: Option<Diff>,
}

#[derive(Clone)]
pub struct Model {
    pub setup: Setup,
    pub info: SystemInfo,
    pub generations: Vec<Generation>,
    /// Packages the current user installed for themselves.
    pub user_packages: Vec<Element>,
    pub configured: Configured,
    /// The current user's name, whose `packages` are read.
    pub user: String,
    /// The system accepts unfree packages.
    pub allow_unfree: bool,
    pub catalog: Vec<Application>,
    /// A flake's inputs, from `flake.lock`.
    pub inputs: Vec<Input>,
    /// A channel system's channels.
    pub channels: Vec<Channel>,
    /// Store path of the system's Nixpkgs, for searching it.
    pub nixpkgs: Option<String>,
    /// The store, once read.
    pub store: Option<Arc<Store>>,
    /// What could not be read, to say so instead of failing.
    pub problems: Vec<String>,
}

impl Default for Model {
    fn default() -> Model {
        Model {
            setup: Setup::detect_in(Path::new("/nonexistent"), Default::default(), ""),
            info: SystemInfo::default(),
            generations: Vec::new(),
            user_packages: Vec::new(),
            configured: Configured::default(),
            user: String::new(),
            allow_unfree: false,
            catalog: Vec::new(),
            inputs: Vec::new(),
            channels: Vec::new(),
            nixpkgs: None,
            store: None,
            problems: Vec::new(),
        }
    }
}

impl Model {
    /// Everything but the store: files Yukimi reads directly, in a few
    /// milliseconds.
    pub fn quick() -> Model {
        let setup = Setup::detect();
        let mut model = Model {
            info: SystemInfo::read(),
            generations: generations::system(),
            user_packages: profile::user_elements(),
            user: std::env::var("USER").unwrap_or_default(),
            catalog: catalog::load(&setup.facts.catalogs),
            nixpkgs: setup.facts.nixpkgs.clone(),
            setup,
            ..Model::default()
        };
        model.read_configuration();
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

    fn read_lock(&mut self) {
        match std::fs::read_to_string(self.setup.dir.join("flake.lock")).map(|t| FlakeLock::parse(&t)) {
            Ok(Ok(lock)) => self.inputs = lock.inputs(),
            Ok(Err(e)) => self.problems.push(e.to_string()),
            Err(_) => {}
        }
    }

    /// This model with the store read.
    pub fn with_store(&self, store: Store, problems: Vec<String>) -> Model {
        let mut model = Model { store: Some(Arc::new(store)), ..self.clone() };
        model.problems.extend(problems);
        model
    }

    pub fn current_generation(&self) -> Option<&Generation> {
        self.generations.iter().find(|g| g.current)
    }

    /// Attributes the current user installed for themselves.
    pub fn user_attrs(&self) -> BTreeSet<&str> {
        self.user_packages.iter().filter_map(|e| e.package_attr()).collect()
    }

    /// Why Yukimi can't change what the system installs, when it can't: it
    /// didn't find the configuration file to add its own to.
    pub fn cannot_change_system(&self) -> Option<String> {
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
    /// installed through, or (for Yukimi's own catalog) all its packages in
    /// the running system however they got there.
    pub fn app_installed(&self, app: &Application, system: &BTreeSet<&str>) -> bool {
        if let Some(setting) = &app.setting {
            return self.configured.settings.get(setting).is_some_and(|ids| ids.contains(&app.id));
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
        // The garbage collector keeps derivations of live paths unless told
        // not to; Nix's default is to keep them.
        let keep_derivations = nix::setting("keep-derivations").is_none_or(|value| value != "false");
        store.composition = composition(&store.graph, &store.roots, keep_derivations);
        store.system_packages = system_packages(&store.graph);
        store.heaviest = heaviest(&store.graph, 24);
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

fn composition(graph: &Graph, roots: &[Root], keep_derivations: bool) -> Composition {
    let mut owner: Vec<u8> = vec![0; graph.len()];
    let groups: [(u8, Vec<&Root>); 4] = [
        (1, roots.iter().filter(|r| matches!(r.kind, RootKind::CurrentSystem | RootKind::BootedSystem)).collect()),
        (2, roots.iter().filter(|r| matches!(r.kind, RootKind::SystemGeneration(_))).collect()),
        (3, roots.iter().filter(|r| matches!(r.kind, RootKind::Profile { .. })).collect()),
        (
            4,
            roots
                .iter()
                .filter(|r| matches!(r.kind, RootKind::BuildResult | RootKind::DevShell | RootKind::Other))
                .collect(),
        ),
    ];
    // If /run/current-system is not among the roots seen, the current system
    // generation stands in for it.
    let current = std::fs::read_link("/run/current-system").ok().and_then(|p| graph.lookup(p.to_str()?));
    for (tag, group) in &groups {
        let mut start = ids(graph, group);
        if *tag == 1 {
            start.extend(current);
        }
        for id in graph.kept(start, keep_derivations) {
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

/// The heaviest packages in the running system's closure, by name.
fn heaviest(graph: &Graph, count: usize) -> Vec<Heavy> {
    let Some(current) = std::fs::read_link("/run/current-system").ok().and_then(|p| graph.lookup(p.to_str()?)) else {
        return Vec::new();
    };
    let mut by_name: HashMap<String, Heavy> = HashMap::new();
    for id in graph.closure([current]) {
        let Some(path) = graph.store_path(id) else { continue };
        if path.is_derivation() {
            continue;
        }
        let entry = by_name.entry(path.name().to_owned()).or_insert_with(|| Heavy {
            name: path.name().to_owned(),
            version: path.version().to_owned(),
            size: 0,
        });
        entry.size += graph.info(id).nar_size;
    }
    let mut list: Vec<Heavy> = by_name.into_values().collect();
    list.sort_by_key(|h| std::cmp::Reverse(h.size));
    list.truncate(count);
    list
}
