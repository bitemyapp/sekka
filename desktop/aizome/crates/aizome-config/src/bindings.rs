//! The command form of a binding, shared by its executor and migrations.
//! Aizome's `spawn` grammar is whitespace-separated arguments, not a shell:
//! quotes, backslashes and metacharacters are literal argument characters.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl SpawnCommand {
    pub fn parse(action: &str) -> Option<Self> {
        let mut words = action.split_whitespace();
        if words.next()? != "spawn" {
            return None;
        }
        Some(Self { program: words.next()?.into(), args: words.map(str::to_owned).collect() })
    }

    pub fn into_argv(self) -> Vec<String> {
        std::iter::once(self.program).chain(self.args).collect()
    }

    /// Encode only values representable by the executor's grammar.
    pub fn render(&self) -> Option<String> {
        let words: Vec<_> =
            std::iter::once(self.program.as_str()).chain(self.args.iter().map(String::as_str)).collect();
        if words.iter().any(|word| word.is_empty() || word.contains(char::is_whitespace)) {
            return None;
        }
        Some(std::iter::once("spawn").chain(words).collect::<Vec<_>>().join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_roundtrip_preserves_executor_arguments() {
        let parsed = SpawnCommand::parse("  spawn\tkoe  record\n toggle --text=$HOME;echo ").unwrap();
        assert_eq!(parsed.clone().into_argv(), ["koe", "record", "toggle", "--text=$HOME;echo"]);
        assert_eq!(SpawnCommand::parse(&parsed.render().unwrap()), Some(parsed));
        assert!(SpawnCommand::parse("spawn").is_none());
        assert!(SpawnCommand::parse("spawned koe").is_none());
        assert!(SpawnCommand { program: "has spaces".into(), args: vec![] }.render().is_none());
    }
}
