//! A small argument parser: flags and options anywhere, `--x=v` or `--x v`,
//! and `--` before arguments that start with a dash.

pub struct Args {
    rest: Vec<String>,
    /// Words after `--`, which are never options.
    literal: Vec<String>,
}

impl Args {
    pub fn new(words: Vec<String>) -> Args {
        match words.iter().position(|w| w == "--") {
            Some(i) => Args { rest: words[..i].to_vec(), literal: words[i + 1..].to_vec() },
            None => Args { rest: words, literal: Vec::new() },
        }
    }

    /// Whether `--name` is given (and take it).
    pub fn flag(&mut self, name: &str) -> bool {
        let before = self.rest.len();
        self.rest.retain(|w| w != name);
        self.rest.len() != before
    }

    /// The value of `--name VALUE` or `--name=VALUE` (and take it).
    pub fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        let prefix = format!("{name}=");
        let mut found = None;
        let mut i = 0;
        while i < self.rest.len() {
            if self.rest[i] == name {
                if i + 1 >= self.rest.len() {
                    return Err(format!("{name} needs a value"));
                }
                found = Some(self.rest.remove(i + 1));
                self.rest.remove(i);
            } else if let Some(v) = self.rest[i].strip_prefix(&prefix) {
                found = Some(v.to_string());
                self.rest.remove(i);
            } else {
                i += 1;
            }
        }
        Ok(found)
    }

    /// The next argument that isn't an option.
    pub fn positional(&mut self) -> Option<String> {
        match self.rest.iter().position(|w| !w.starts_with('-') || w == "-") {
            Some(i) => Some(self.rest.remove(i)),
            None if !self.literal.is_empty() => Some(self.literal.remove(0)),
            None => None,
        }
    }

    /// Fails on anything left over.
    pub fn finish(self) -> Result<(), String> {
        match self.rest.first().or(self.literal.first()) {
            Some(w) if w.starts_with('-') => Err(format!("unknown option {w}")),
            Some(w) => Err(format!("unexpected argument {w:?}")),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Args {
        Args::new(s.split_whitespace().map(str::to_string).collect())
    }

    #[test]
    fn options_go_anywhere() {
        let mut a = args("--json --role=fast");
        assert!(a.flag("--json"));
        assert_eq!(a.value("--role").unwrap().as_deref(), Some("fast"));
        assert!(a.finish().is_ok());
        let mut a = args("download --quiet gemma-4-31b");
        assert_eq!(a.positional().as_deref(), Some("download"));
        assert!(a.flag("--quiet"));
        assert_eq!(a.positional().as_deref(), Some("gemma-4-31b"));
        assert!(a.finish().is_ok());
        let mut a = args("--role");
        assert!(a.value("--role").is_err());
        let mut a = args("x --nope");
        assert_eq!(a.positional().as_deref(), Some("x"));
        assert_eq!(a.finish().unwrap_err(), "unknown option --nope");
        let mut a = args("-- -weird");
        assert_eq!(a.positional().as_deref(), Some("-weird"));
    }
}
