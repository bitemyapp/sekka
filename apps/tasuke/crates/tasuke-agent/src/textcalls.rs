//! Tool calls a model wrote in its message text instead of the API's
//! `tool_calls`, as some chat templates leave them: `<tool_call>{…}</tool_call>`,
//! a fenced JSON object, or the whole message as one JSON object. Ported
//! from tests/agent-evals/harness/agent.py (`text_tool_calls`).

use serde_json::Value;

/// A call found in text: the tool's name and its arguments as JSON text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TextCall {
    pub name: String,
    pub arguments: String,
}

pub(crate) fn find(content: &str, names: &[&str]) -> Vec<TextCall> {
    if content.trim().is_empty() {
        return Vec::new();
    }
    for blocks in [tagged(content), fenced(content)] {
        let found: Vec<TextCall> = blocks.iter().filter_map(|b| call_of(b, names)).collect();
        if !found.is_empty() {
            return found;
        }
    }
    let s = content.trim();
    if s.starts_with('{') && s.ends_with('}') {
        return call_of(s, names).into_iter().collect();
    }
    Vec::new()
}

/// The JSON objects between `<tool_call>` and `</tool_call>`.
fn tagged(content: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("<tool_call>") {
        let after = &rest[start + "<tool_call>".len()..];
        let Some(end) = after.find("</tool_call>") else { break };
        out.push(after[..end].trim());
        rest = &after[end + "</tool_call>".len()..];
    }
    out
}

/// The insides of ``` fences (optionally ```json) that hold one object.
fn fenced(content: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after);
        let Some(end) = after.find("```") else { break };
        let inside = after[..end].trim();
        if inside.starts_with('{') && inside.ends_with('}') {
            out.push(inside);
        }
        rest = &after[end + 3..];
    }
    out
}

fn call_of(text: &str, names: &[&str]) -> Option<TextCall> {
    let obj: Value = serde_json::from_str(text).ok()?;
    let name = obj.get("name").or_else(|| obj.get("function")).and_then(Value::as_str)?;
    if !names.contains(&name) {
        return None;
    }
    let args = obj.get("arguments").or_else(|| obj.get("parameters"));
    let arguments = match args {
        None => "{}".to_owned(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    };
    Some(TextCall { name: name.to_owned(), arguments })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: &[&str] = &["run_command", "read_file"];

    fn names(calls: &[TextCall]) -> Vec<&str> {
        calls.iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn tagged_calls() {
        let c = find(
            "I'll check.\n<tool_call>\n{\"name\": \"run_command\", \"arguments\": {\"command\": \"ls\"}}\n</tool_call>\
             <tool_call>{\"name\":\"read_file\",\"arguments\":{\"path\":\"/etc/hosts\"}}</tool_call>",
            NAMES,
        );
        assert_eq!(names(&c), ["run_command", "read_file"]);
        assert_eq!(c[0].arguments, r#"{"command":"ls"}"#);
    }

    #[test]
    fn fenced_and_bare_calls() {
        let c = find("```json\n{\"name\": \"run_command\", \"parameters\": {\"command\": \"df -h\"}}\n```", NAMES);
        assert_eq!(c, [TextCall { name: "run_command".into(), arguments: r#"{"command":"df -h"}"#.into() }]);
        let c = find("  {\"function\": \"read_file\", \"arguments\": \"{\\\"path\\\": \\\"/x\\\"}\"}  ", NAMES);
        assert_eq!(c[0].arguments, r#"{"path": "/x"}"#);
        let c = find("{\"name\": \"read_file\"}", NAMES);
        assert_eq!(c[0].arguments, "{}");
    }

    #[test]
    fn not_calls() {
        assert!(find("", NAMES).is_empty());
        assert!(find("The answer is {x}.", NAMES).is_empty());
        assert!(find("<tool_call>{\"name\": \"rm_everything\"}</tool_call>", NAMES).is_empty());
        assert!(find("```json\n{\"name\": \"run_command\", oops}\n```", NAMES).is_empty());
        assert!(find("Here is your config:\n```json\n{\"theme\": \"day\"}\n```", NAMES).is_empty());
        assert!(find("<tool_call>{\"name\":\"run_command\"}", NAMES).is_empty(), "unterminated");
    }
}
