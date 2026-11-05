//! Skills: `SKILL.md` knowledge files, one per directory, with YAML front
//! matter (`name`, `description`). The system prompt lists their names and
//! descriptions; `load_skill` reads one into the conversation.

use std::path::{Path, PathBuf};

/// The most of a skill that enters the context.
const MAX_SKILL_BYTES: usize = 48 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// The skills found, by name; the first directory listed wins a name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Skills {
    skills: Vec<Skill>,
}

impl Skills {
    /// The directories in `$TASUKE_SKILLS_DIRS` (colon-separated), else [`Skills::default_dirs`].
    pub fn discover() -> Skills {
        let dirs: Vec<PathBuf> = match std::env::var_os("TASUKE_SKILLS_DIRS").filter(|v| !v.is_empty()) {
            Some(v) => std::env::split_paths(&v).collect(),
            None => Skills::default_dirs(),
        };
        Skills::discover_in(&dirs)
    }

    /// The installed share directory beside the executable
    /// (`<prefix>/share/tasuke/skills`), each of `$XDG_DATA_DIRS`, and in a
    /// development build the workspace's `skills/`.
    pub fn default_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(prefix) = std::env::current_exe().ok().and_then(|e| e.parent()?.parent().map(Path::to_path_buf)) {
            dirs.push(prefix.join("share/tasuke/skills"));
        }
        let data_dirs = std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/run/current-system/sw/share".into());
        for d in std::env::split_paths(&data_dirs) {
            dirs.push(d.join("tasuke/skills"));
        }
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        if dev.is_dir() {
            dirs.push(dev);
        }
        dirs
    }

    pub fn discover_in(dirs: &[PathBuf]) -> Skills {
        let mut skills: Vec<Skill> = Vec::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            let mut found: Vec<Skill> = entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let file = e.path().join("SKILL.md");
                    let text = std::fs::read_to_string(&file).ok()?;
                    let fallback = e.file_name().to_string_lossy().into_owned();
                    let (name, description) = front_matter(&text, &fallback)?;
                    Some(Skill { name, description, path: file })
                })
                .collect();
            found.sort_by(|a, b| a.name.cmp(&b.name));
            for s in found {
                if !skills.iter().any(|k| k.name == s.name) {
                    skills.push(s);
                }
            }
        }
        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Skills { skills }
    }

    pub fn index(&self) -> &[Skill] {
        &self.skills
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        let name = name.trim().trim_end_matches("/SKILL.md");
        self.skills.iter().find(|s| s.name == name || s.name.eq_ignore_ascii_case(name))
    }

    /// A skill's text without its front matter, capped.
    pub fn load(&self, name: &str) -> Result<String, String> {
        let Some(skill) = self.get(name) else {
            let names: Vec<&str> = self.skills.iter().map(|s| s.name.as_str()).collect();
            return Err(format!("there is no skill named {name:?}; the skills are: {}", names.join(", ")));
        };
        let text = std::fs::read_to_string(&skill.path).map_err(|e| format!("{}: {e}", skill.path.display()))?;
        let mut body = strip_front_matter(&text).trim_start().to_owned();
        if body.len() > MAX_SKILL_BYTES {
            let mut cut = MAX_SKILL_BYTES;
            while !body.is_char_boundary(cut) {
                cut -= 1;
            }
            body.truncate(cut);
            body.push_str("\n… [the rest of this skill is cut off]");
        }
        Ok(body)
    }

    /// The index as the system prompt lists it.
    pub fn prompt_block(&self) -> String {
        self.skills.iter().map(|s| format!("- {}: {}", s.name, s.description)).collect::<Vec<_>>().join("\n")
    }
}

fn strip_front_matter(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---") else { return text };
    let rest = rest.trim_start_matches([' ', '\t']);
    let Some(rest) = rest.strip_prefix('\n').or_else(|| rest.strip_prefix("\r\n")) else { return text };
    for (i, _) in rest.match_indices("\n---") {
        let after = &rest[i + 4..];
        if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
            return after.trim_start_matches(['\r', '\n']);
        }
    }
    text
}

/// `name` and `description` from YAML front matter: plain, quoted, folded
/// (`>`) or literal (`|`) scalars, and plain values continued on indented
/// lines. A skill without a description is skipped.
fn front_matter(text: &str, fallback_name: &str) -> Option<(String, String)> {
    let body = text.strip_prefix("---")?;
    let end = body.find("\n---")?;
    let lines: Vec<&str> = body[..end].lines().collect();
    let mut name = None;
    let mut description = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.starts_with([' ', '\t']) || line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        let mut cont = Vec::new();
        while i < lines.len() && (lines[i].starts_with([' ', '\t']) || lines[i].trim().is_empty()) {
            cont.push(lines[i].trim());
            i += 1;
        }
        let text = if value.starts_with('>') || value.starts_with('|') {
            let sep = if value.starts_with('>') { " " } else { "\n" };
            cont.iter().copied().filter(|l| !l.is_empty()).collect::<Vec<_>>().join(sep)
        } else {
            let mut all = vec![value];
            all.extend(cont.iter().copied().filter(|l| !l.is_empty()));
            unquote(&all.join(" "))
        };
        match key.trim() {
            "name" => name = Some(text),
            "description" => description = Some(text),
            _ => {}
        }
    }
    let name = name.filter(|n| !n.is_empty()).unwrap_or_else(|| fallback_name.to_owned());
    let description = description.filter(|d| !d.is_empty())?;
    Some((name, description))
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return s[1..s.len() - 1].replace("\\\"", "\"").replace("\\n", "\n");
    }
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("''", "'");
    }
    s.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dirs: &[(&str, &[(&str, &str)])]) -> (PathBuf, Vec<PathBuf>) {
        let root = std::env::temp_dir().join(format!("tasuke-skills-{}", crate::ids::random_hex(4)));
        let mut out = Vec::new();
        for (dir, skills) in dirs {
            let d = root.join(dir);
            for (name, text) in *skills {
                std::fs::create_dir_all(d.join(name)).unwrap();
                std::fs::write(d.join(name).join("SKILL.md"), text).unwrap();
            }
            std::fs::create_dir_all(&d).unwrap();
            out.push(d);
        }
        (root, out)
    }

    #[test]
    fn discovers_skills_across_directories_first_wins() {
        let (root, dirs) = fixture(&[
            (
                "installed",
                &[
                    (
                        "aizome-settings",
                        "---\nname: aizome-settings\ndescription: Settings, panes and keys.\n---\n# A\n\nBody.",
                    ),
                    ("sekka-basics", "---\nname: sekka-basics\ndescription: \"How Sekka: works\"\n---\nBasics."),
                    ("broken", "no front matter"),
                ],
            ),
            ("dev", &[("aizome-settings", "---\nname: aizome-settings\ndescription: the dev copy\n---\nDev.")]),
        ]);
        let mut missing = dirs.clone();
        missing.push(root.join("missing"));
        let skills = Skills::discover_in(&missing);
        let names: Vec<&str> = skills.index().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["aizome-settings", "sekka-basics"]);
        assert_eq!(skills.get("aizome-settings").unwrap().description, "Settings, panes and keys.");
        assert_eq!(skills.get("sekka-basics").unwrap().description, "How Sekka: works");
        assert_eq!(skills.load("aizome-settings").unwrap(), "# A\n\nBody.");
        assert_eq!(skills.load("Sekka-Basics").unwrap(), "Basics.");
        let err = skills.load("nope").unwrap_err();
        assert!(err.contains("aizome-settings, sekka-basics"), "{err}");
        assert!(skills.prompt_block().contains("- sekka-basics: How Sekka: works"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn env_var_names_the_directories() {
        let (root, dirs) = fixture(&[
            ("one", &[("x", "---\nname: x\ndescription: d\n---\n")]),
            ("two", &[("y", "---\ndescription: e\n---\n")]),
        ]);
        let joined = std::env::join_paths(&dirs).unwrap();
        // SAFETY: tests that touch the environment use this variable only.
        unsafe { std::env::set_var("TASUKE_SKILLS_DIRS", &joined) };
        let skills = Skills::discover();
        unsafe { std::env::remove_var("TASUKE_SKILLS_DIRS") };
        let names: Vec<&str> = skills.index().iter().map(|s| s.name.as_str()).collect();
        // `y` has no name: its directory names it.
        assert_eq!(names, ["x", "y"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn front_matter_forms() {
        let folded = "---\nname: a\ndescription: >-\n  Folded over\n  two lines.\n---\nbody";
        assert_eq!(front_matter(folded, "f"), Some(("a".into(), "Folded over two lines.".into())));
        let plain = "---\nname: b\ndescription: Plain and\n  continued.\nother: x\n---\n";
        assert_eq!(front_matter(plain, "f"), Some(("b".into(), "Plain and continued.".into())));
        let single = "---\nname: 'c'\ndescription: 'It''s quoted'\n---\n";
        assert_eq!(front_matter(single, "f"), Some(("c".into(), "It's quoted".into())));
        assert_eq!(front_matter("---\nname: d\n---\n", "f"), None);
        assert_eq!(strip_front_matter("---\nname: x\n---\n\nHello"), "Hello");
        assert_eq!(strip_front_matter("Hello"), "Hello");
    }

    #[test]
    fn real_skills_parse_when_present() {
        // The skills being written for the assistant, when this machine has them.
        let dir = PathBuf::from("/home/callen/work/sekka-open-model-evals/assistant/skills");
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        for d in [dir, dev] {
            let skills = Skills::discover_in(std::slice::from_ref(&d));
            for s in skills.index() {
                assert!(!s.description.is_empty(), "{}", s.name);
                assert!(!s.description.starts_with('>'), "{}", s.name);
                assert!(skills.load(&s.name).unwrap().len() > 100, "{}", s.name);
            }
        }
    }
}
