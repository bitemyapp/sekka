use crate::config::Replacement;
use anyhow::Result;
use regex::{NoExpand, Regex};

pub struct Processor {
    rules: Vec<(Regex, String)>,
    append_space: bool,
}
impl Processor {
    pub fn new(replacements: &[Replacement], append_space: bool) -> Result<Self> {
        let rules = replacements
            .iter()
            .map(|r| {
                // Unicode word boundaries prevent a replacement inside another word.
                Ok((Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&r.from)))?, r.to.clone()))
            })
            .collect::<Result<_>>()?;
        Ok(Self { rules, append_space })
    }
    pub fn apply(&self, text: &str) -> String {
        let mut text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        for (re, to) in &self.rules {
            text = re.replace_all(&text, NoExpand(to)).into_owned();
        }
        if self.append_space && !text.is_empty() {
            text.push(' ');
        }
        text
    }
}
