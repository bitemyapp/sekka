use anyhow::{Context, Result, bail, ensure};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const REPO: &str = "bitemyapp/sekka";
const REPO_ID: u64 = 1412948279;
const WEB: &str = "https://github.com/bitemyapp/sekka";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub actor: Value,
    #[serde(default)]
    pub repo: Value,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub key: String,
    pub occurred_at: i64,
    pub message: Value,
}

pub fn marker(key: &str) -> String {
    format!("yuki:{:x}", Sha256::digest(key.as_bytes()))
}

pub fn stamp_message(message: &mut Value, key: &str) {
    let marker = marker(key);
    if let Some(embeds) = message["embeds"].as_array_mut() {
        for embed in embeds {
            embed["footer"] = json!({"text": format!("{REPO} · GitHub · {marker}")});
        }
    }
    message["nonce"] = json!(&marker[5..29]);
    message["enforce_nonce"] = json!(true);
}

pub fn normalize_event(event: &Event) -> Result<Option<Notification>> {
    if !is_public(event) {
        return Ok(None);
    }
    if let Some(id) = event.repo.get("id") {
        ensure!(
            id.as_u64() == Some(REPO_ID),
            "unexpected feed repository ID"
        );
    }
    normalize(event, false)
}

pub fn normalize_webhook(kind: &str, payload: &Value) -> Result<Option<Notification>> {
    let kind = match kind {
        "pull_request" => "PullRequestEvent",
        "issues" => "IssuesEvent",
        "issue_comment" => "IssueCommentEvent",
        "pull_request_review" => "PullRequestReviewEvent",
        "pull_request_review_comment" => "PullRequestReviewCommentEvent",
        "push" => "PushEvent",
        "release" => "ReleaseEvent",
        "discussion" => "DiscussionEvent",
        "discussion_comment" => "DiscussionCommentEvent",
        "create" => "CreateEvent",
        "delete" => "DeleteEvent",
        "gollum" => "GollumEvent",
        "public" => "PublicEvent",
        _ => return Ok(None),
    };
    let repository = &payload["repository"];
    let private = repository["private"]
        .as_bool()
        .context("missing repository.private")?;
    if private {
        return Ok(None);
    }
    let name = required_text(&repository["full_name"], "repository.full_name")?;
    let id = positive_id(&repository["id"], "repository.id")?;
    if name != REPO || id != REPO_ID {
        return Ok(None);
    }
    let mut event = Event {
        id: String::new(),
        kind: kind.into(),
        actor: payload["sender"].clone(),
        repo: json!({"name": REPO, "id": REPO_ID}),
        payload: payload.clone(),
        public: true,
        created_at: String::new(),
    };
    if kind == "PushEvent" {
        event.payload["head"] = payload["after"].clone();
        if let Some(commits) = event.payload["commits"].as_array_mut() {
            for commit in commits {
                commit["sha"] = commit["id"].clone();
            }
        }
    }
    if kind == "PullRequestReviewEvent" && payload["action"] == "submitted" {
        event.payload["action"] = json!("created");
    }
    normalize(&event, true)
}

fn normalize(event: &Event, webhook: bool) -> Result<Option<Notification>> {
    let p = &event.payload;
    let fallback = if webhook {
        None
    } else {
        Some(event.created_at.as_str())
    };
    let action = || required_text(&p["action"], "action");
    let number = || {
        issue_number(event)
            .filter(|n| *n > 0)
            .context("missing issue/PR number")
    };
    let (identity, occurred_at) = match event.kind.as_str() {
        "PullRequestEvent" | "IssuesEvent" => {
            let pr = event.kind == "PullRequestEvent";
            let object = if pr { &p["pull_request"] } else { &p["issue"] };
            let mut action = action()?;
            if !matches!(action, "opened" | "closed" | "merged" | "reopened") {
                return Ok(None);
            }
            ensure!(object.is_object(), "missing issue/PR object");
            if pr && action == "closed" {
                match object["merged"].as_bool() {
                    Some(true) => action = "merged",
                    Some(false) => {}
                    // Reduced Events API uses a separate `merged` action.
                    None if !webhook && object.get("merged").is_none() => {}
                    None => bail!("missing pull_request.merged on closed webhook"),
                }
            }
            ensure!(pr || action != "merged", "issue cannot be merged");
            let field = match action {
                "opened" => "created_at",
                "merged" => "merged_at",
                "closed" => "closed_at",
                _ => "updated_at",
            };
            let at = occurrence(&object[field], fallback)?;
            let kind = if pr { "pr" } else { "issue" };
            let mut key = format!("{kind}:{}:{action}", number()?);
            if matches!(action, "closed" | "reopened") {
                key.push_str(&format!(":{at}"));
            }
            (key, at)
        }
        "IssueCommentEvent" | "PullRequestReviewCommentEvent" | "DiscussionCommentEvent" => {
            let action = action()?;
            if !matches!(action, "created" | "edited" | "deleted") {
                return Ok(None);
            }
            // Deletion snapshots retain the last edit time, not deletion time.
            // The terminal ID+deleted identity does not depend on receipt time.
            if event.kind == "DiscussionCommentEvent" {
                positive_id(&p["discussion"]["number"], "discussion.number")?;
            } else {
                number()?;
            }
            let id = positive_id(&p["comment"]["id"], "comment.id")?;
            let at = if action == "deleted" {
                receipt_or_feed_time(fallback)?
            } else {
                occurrence(
                    &p["comment"][if action == "created" {
                        "created_at"
                    } else {
                        "updated_at"
                    }],
                    fallback,
                )?
            };
            let kind = match event.kind.as_str() {
                "IssueCommentEvent" => "issue-comment",
                "PullRequestReviewCommentEvent" => "review-comment",
                _ => "discussion-comment",
            };
            let mut key = format!("{kind}:{id}:{action}");
            if action == "edited" {
                key.push_str(&format!(":{at}"));
            }
            (key, at)
        }
        "PullRequestReviewEvent" => {
            let action = action()?;
            if !matches!(action, "created" | "dismissed") {
                // Reviews have submitted_at, but no shared edit timestamp.
                return Ok(None);
            }
            number()?;
            let id = positive_id(&p["review"]["id"], "review.id")?;
            let at = if action == "created" {
                occurrence(&p["review"]["submitted_at"], fallback)?
            } else {
                receipt_or_feed_time(fallback)?
            };
            (format!("review:{id}:{action}"), at)
        }
        "PushEvent" => {
            let reference = required_text(&p["ref"], "ref")?;
            ensure!(reference.starts_with("refs/"), "invalid push ref");
            let before = required_text(&p["before"], "before")?;
            let head = required_text(&p["head"], "head/after")?;
            ensure!(
                valid_sha(before) && valid_sha(head),
                "invalid push commit SHA"
            );
            let at = if webhook {
                push_time(&p["repository"]["pushed_at"])?
            } else {
                occurrence(&Value::Null, fallback)?
            };
            // The webhook has no Events API push_id. Identical A->B force-push
            // cycles are indistinguishable; prefer suppressing a duplicate.
            (
                format!(
                    "push:{}:{}:{}",
                    segment(reference),
                    before.to_ascii_lowercase(),
                    head.to_ascii_lowercase()
                ),
                at,
            )
        }
        "ReleaseEvent" => {
            let action = action()?;
            if !matches!(action, "published" | "released" | "prereleased") {
                return Ok(None);
            }
            let release = &p["release"];
            if release["draft"].as_bool() == Some(true) {
                return Ok(None);
            }
            let id = positive_id(&release["id"], "release.id")?;
            let at = occurrence(&release["published_at"], fallback)?;
            (format!("release:{id}:{action}"), at)
        }
        "DiscussionEvent" => {
            let action = action()?;
            if !matches!(action, "created" | "closed" | "reopened") {
                return Ok(None);
            }
            let discussion = &p["discussion"];
            let number = positive_id(&discussion["number"], "discussion.number")?;
            let field = match action {
                "created" => "created_at",
                "closed" => "closed_at",
                _ => "updated_at",
            };
            let at = occurrence(&discussion[field], fallback)?;
            let mut key = format!("discussion:{number}:{action}");
            if action != "created" {
                key.push_str(&format!(":{at}"));
            }
            (key, at)
        }
        "CreateEvent" | "DeleteEvent" => {
            let kind = required_text(&p["ref_type"], "ref_type")?;
            ensure!(
                matches!(kind, "branch" | "tag" | "repository"),
                "invalid ref_type"
            );
            ensure!(
                kind != "repository" || event.kind == "CreateEvent",
                "cannot delete repository ref"
            );
            let reference = if kind == "repository" {
                ""
            } else {
                required_text(&p["ref"], "ref")?
            };
            let at = receipt_or_feed_time(fallback)?;
            // No event time/ID is shared by these webhook and feed schemas.
            // Repeated creation/deletion of the same ref is deliberately coalesced.
            (format!("{}:{kind}:{}", event.kind, segment(reference)), at)
        }
        "GollumEvent" => {
            let pages = p["pages"].as_array().context("missing wiki pages")?;
            ensure!(!pages.is_empty(), "empty wiki pages");
            let mut identities = Vec::with_capacity(pages.len());
            for page in pages {
                let name = required_text(&page["page_name"], "page_name")?;
                let sha = required_text(&page["sha"], "page.sha")?;
                ensure!(valid_sha(sha), "invalid wiki SHA");
                identities.push(format!("{}:{}", segment(name), sha.to_ascii_lowercase()));
            }
            identities.sort_unstable();
            identities.dedup();
            (
                format!("wiki:{}", identities.join(",")),
                receipt_or_feed_time(fallback)?,
            )
        }
        "PublicEvent" => {
            let at = receipt_or_feed_time(fallback)?;
            ("public".to_owned(), at)
        }
        _ => return Ok(None),
    };
    let key = format!("{REPO}:{identity}");
    let mut message = render_unstamped(event).context("normalized event has no renderer")?;
    stamp_message(&mut message, &key);
    Ok(Some(Notification {
        key,
        occurred_at,
        message,
    }))
}

fn receipt_or_feed_time(feed_time: Option<&str>) -> Result<i64> {
    match feed_time {
        Some(at) => timestamp(at),
        // Used only where the webhook schema has no event timestamp. This is
        // cutoff metadata, NEVER identity: redeliveries retain the same key.
        None => Ok(chrono::Utc::now().timestamp()),
    }
}

fn required_text<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing or invalid {name}"))
}

fn positive_id(value: &Value, name: &str) -> Result<u64> {
    value
        .as_u64()
        .filter(|id| *id > 0)
        .with_context(|| format!("missing or invalid {name}"))
}

fn timestamp(value: &str) -> Result<i64> {
    ensure!(value.len() <= 64, "timestamp too long");
    let at = DateTime::parse_from_rfc3339(value)
        .context("invalid RFC3339 timestamp")?
        .timestamp();
    ensure!(at >= 0, "timestamp before Unix epoch");
    Ok(at)
}

fn occurrence(value: &Value, fallback: Option<&str>) -> Result<i64> {
    if value.is_null() {
        timestamp(fallback.context("missing occurrence timestamp")?)
    } else {
        timestamp(required_text(value, "occurrence timestamp")?)
    }
}

fn push_time(value: &Value) -> Result<i64> {
    if let Some(at) = value.as_i64() {
        ensure!(
            at >= 0 && DateTime::from_timestamp(at, 0).is_some(),
            "invalid push timestamp"
        );
        Ok(at)
    } else {
        timestamp(required_text(value, "repository.pushed_at")?)
    }
}

pub fn issue_number(event: &Event) -> Option<u64> {
    let p = &event.payload;
    match event.kind.as_str() {
        "PullRequestEvent" | "PullRequestReviewEvent" | "PullRequestReviewCommentEvent" => {
            p["number"]
                .as_u64()
                .or_else(|| p["pull_request"]["number"].as_u64())
        }
        "IssuesEvent" | "IssueCommentEvent" => p["issue"]["number"].as_u64(),
        _ => None,
    }
}

pub fn is_public(event: &Event) -> bool {
    event.public && event.repo["name"].as_str() == Some(REPO)
}

pub fn render(event: &Event) -> Option<Value> {
    let mut message = render_unstamped(event)?;
    stamp_message(&mut message, &event.id);
    Some(message)
}

fn render_unstamped(event: &Event) -> Option<Value> {
    if !is_public(event) {
        return None;
    }
    let p = &event.payload;
    let actor = text(&event.actor["login"], "Someone");
    let action = text(&p["action"], "updated");
    let number = issue_number(event);
    let item = |kind: &str, title: &Value| {
        let suffix = title
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| format!(": {s}"))
            .unwrap_or_default();
        format!(
            "{kind} #{}{suffix}",
            number.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
        )
    };
    let (title, description, url) = match event.kind.as_str() {
        "PullRequestEvent" => {
            let action = if action == "merged"
                || (action == "closed" && p["pull_request"]["merged"].as_bool() == Some(true))
            {
                "merged"
            } else {
                action
            };
            (
                format!(
                    "{actor} {action} {}",
                    item("PR", &p["pull_request"]["title"])
                ),
                String::new(),
                item_url("pull", number),
            )
        }
        "IssuesEvent" => (
            format!("{actor} {action} {}", item("issue", &p["issue"]["title"])),
            String::new(),
            item_url("issues", number),
        ),
        "IssueCommentEvent" => {
            let path = if p["issue"].get("pull_request").is_some() {
                "pull"
            } else {
                "issues"
            };
            let url = anchored(item_url(path, number), "issuecomment", &p["comment"]["id"]);
            (
                format!(
                    "{actor} {action} comment on {}",
                    item(
                        if path == "pull" { "PR" } else { "issue" },
                        &p["issue"]["title"]
                    )
                ),
                text(&p["comment"]["body"], "").to_owned(),
                url,
            )
        }
        "PullRequestReviewEvent" => (
            format!(
                "{actor} {action} review on {}",
                item("PR", &p["pull_request"]["title"])
            ),
            format!(
                "{}\n{}",
                text(&p["review"]["state"], "Review updated"),
                text(&p["review"]["body"], "")
            ),
            anchored(
                item_url("pull", number),
                "pullrequestreview",
                &p["review"]["id"],
            ),
        ),
        "PullRequestReviewCommentEvent" => (
            format!(
                "{actor} {action} review comment on {}",
                item("PR", &p["pull_request"]["title"])
            ),
            text(&p["comment"]["body"], "").to_owned(),
            anchored(
                item_url("pull", number),
                "discussion_r",
                &p["comment"]["id"],
            ),
        ),
        "ReleaseEvent" => {
            let release = &p["release"];
            let tag = text(&release["tag_name"], "");
            (
                format!("{actor} {action} release {}", text(&release["name"], tag)),
                text(&release["body"], "").to_owned(),
                if tag.is_empty() {
                    format!("{WEB}/releases")
                } else {
                    format!("{WEB}/releases/tag/{}", segment(tag))
                },
            )
        }
        "PushEvent" => {
            let reference = text(&p["ref"], "unknown ref");
            let branch = reference
                .strip_prefix("refs/heads/")
                .or_else(|| reference.strip_prefix("refs/tags/"))
                .unwrap_or(reference);
            let before = p["before"].as_str().filter(|s| valid_sha(s));
            let head = p["head"].as_str().filter(|s| valid_sha(s));
            let url = match (before, head) {
                (Some(before), Some(head))
                    if !before.bytes().all(|b| b == b'0') && !head.bytes().all(|b| b == b'0') =>
                {
                    format!("{WEB}/compare/{before}...{head}")
                }
                (_, Some(head)) if !head.bytes().all(|b| b == b'0') => {
                    format!("{WEB}/commit/{head}")
                }
                _ => format!("{WEB}/commits/{}", segment(branch)),
            };
            let commits: Vec<String> = p["commits"]
                .as_array()
                .into_iter()
                .flatten()
                .take(8)
                .map(|c| {
                    let sha = text(&c["sha"], "").chars().take(7).collect::<String>();
                    let message = text(&c["message"], "Commit")
                        .lines()
                        .next()
                        .unwrap_or("Commit");
                    format!("{sha} {}", truncate(message, 300))
                })
                .collect();
            (
                format!("{actor} pushed to {branch}"),
                commits.join("\n"),
                url,
            )
        }
        "DiscussionEvent" | "DiscussionCommentEvent" => {
            let discussion = &p["discussion"];
            let number = discussion["number"].as_u64();
            let url = if event.kind == "DiscussionCommentEvent" {
                anchored(
                    item_url("discussions", number),
                    "discussioncomment",
                    &p["comment"]["id"],
                )
            } else {
                item_url("discussions", number)
            };
            (
                format!(
                    "{actor} {action} {}discussion #{}: {}",
                    if event.kind == "DiscussionCommentEvent" {
                        "comment on "
                    } else {
                        ""
                    },
                    number.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                    text(&discussion["title"], "")
                ),
                text(
                    if event.kind == "DiscussionCommentEvent" {
                        &p["comment"]["body"]
                    } else {
                        &discussion["body"]
                    },
                    "",
                )
                .to_owned(),
                url,
            )
        }
        "CreateEvent" | "DeleteEvent" => {
            let reference = text(&p["ref"], "repository");
            let kind = text(&p["ref_type"], "ref");
            let deleted = event.kind == "DeleteEvent";
            (
                format!(
                    "{actor} {} {kind} {reference}",
                    if deleted { "deleted" } else { "created" }
                ),
                String::new(),
                if deleted || kind == "repository" {
                    WEB.to_owned()
                } else {
                    format!("{WEB}/tree/{}", segment(reference))
                },
            )
        }
        "GollumEvent" => (
            format!("{actor} updated the wiki"),
            String::new(),
            format!("{WEB}/wiki"),
        ),
        "PublicEvent" => (
            format!("{actor} made the repository public"),
            String::new(),
            WEB.to_owned(),
        ),
        // Stars, forks, and unknown event schemas are deliberately not announced.
        _ => return None,
    };
    Some(message(&title, &description, &url, None))
}

pub fn message(title: &str, description: &str, url: &str, key: Option<&str>) -> Value {
    let url = if url.len() <= 2048 { url } else { WEB };
    let mut message = json!({
        "allowed_mentions": {"parse": [], "users": [], "roles": [], "replied_user": false},
        "embeds": [{"title": truncate(title, 256), "description": truncate(description, 4096), "url": url, "color": 6584745,
                    "footer": {"text": "bitemyapp/sekka · GitHub"}}]
    });
    if let Some(key) = key {
        stamp_message(&mut message, key);
    }
    message
}

fn text<'a>(value: &'a Value, fallback: &'a str) -> &'a str {
    value.as_str().unwrap_or(fallback)
}

fn item_url(kind: &str, number: Option<u64>) -> String {
    match number {
        Some(number) => format!("{WEB}/{kind}/{number}"),
        None if kind == "pull" => format!("{WEB}/pulls"),
        None => format!("{WEB}/{kind}"),
    }
}

fn anchored(mut url: String, prefix: &str, id: &Value) -> String {
    if let Some(id) = id.as_u64() {
        let separator = if prefix == "discussion_r" { "" } else { "-" };
        url.push_str(&format!("#{prefix}{separator}{id}"));
    }
    url
}

fn valid_sha(sha: &str) -> bool {
    (sha.len() == 40 || sha.len() == 64) && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

fn segment(value: &str) -> String {
    let mut result = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(b));
        } else {
            use std::fmt::Write;
            let _ = write!(result, "%{b:02X}");
        }
    }
    result
}

// Count UTF-16 units conservatively: Discord enforces character limits and
// supplementary Unicode characters must never make a valid message too long.
fn truncate(value: &str, limit: usize) -> String {
    let mut units = 0;
    value
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= limit
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn event(id: &str, kind: &str, payload: Value) -> Event {
        Event {
            id: id.into(),
            kind: kind.into(),
            actor: json!({"login": "alice"}),
            repo: json!({"name": REPO}),
            public: true,
            created_at: "2026-11-05T02:42:55Z".into(),
            payload,
        }
    }

    const TIME: &str = "2026-11-05T02:42:55Z";
    const LATER: &str = "2026-11-05T02:43:55Z";

    fn webhook(mut payload: Value) -> Value {
        payload["repository"] = json!({
            "id": REPO_ID, "full_name": REPO, "private": false,
            "pushed_at": timestamp(TIME).unwrap()
        });
        payload["sender"] = json!({"login": "renamed-webhook-actor"});
        payload
    }

    fn pair(kind: &str, feed_kind: &str, hook: Value, feed: Value) -> (Notification, Notification) {
        let hook = normalize_webhook(kind, &webhook(hook)).unwrap().unwrap();
        let feed = normalize_event(&event("unrelated-feed-id", feed_kind, feed))
            .unwrap()
            .unwrap();
        assert_eq!(hook.key, feed.key);
        assert_eq!(hook.occurred_at, feed.occurred_at);
        assert_eq!(hook.message["nonce"], feed.message["nonce"]);
        assert_eq!(
            hook.message["embeds"][0]["footer"],
            feed.message["embeds"][0]["footer"]
        );
        (hook, feed)
    }

    #[test]
    fn rich_merge_matches_actual_reduced_feed_and_opened_is_once_per_pr() {
        let (hook, feed) = pair(
            "pull_request",
            "PullRequestEvent",
            json!({"action":"closed","number":258,"pull_request":{
                "number":258,"merged":true,"merged_at":TIME,"title":"Rich title"
            }}),
            json!({"action":"merged","number":258,"pull_request":{
                "id":5165212809u64,"number":258,"head":{"sha":"a".repeat(40)}
            }}),
        );
        assert_eq!(hook.key, format!("{REPO}:pr:258:merged"));
        assert_ne!(
            hook.message["embeds"][0]["title"],
            feed.message["embeds"][0]["title"]
        );
        let (_, opened) = pair(
            "pull_request",
            "PullRequestEvent",
            json!({"action":"opened","number":286,"pull_request":{
                "number":286,"created_at":TIME,"updated_at":LATER,"title":"@everyone"
            }}),
            json!({"action":"opened","number":286,"pull_request":{
                "url":"https://api.github.com/repos/bitemyapp/sekka/pulls/286",
                "id":5165212809u64,"number":286,
                "head":{"ref":"docs/clarify-command-working-directory","sha":"a".repeat(40)},
                "base":{"ref":"main","sha":"b".repeat(40)}
            }}),
        );
        let mut later = event(
            "different-id",
            "PullRequestEvent",
            json!({
                "action":"opened","number":286,"pull_request":{"number":286}
            }),
        );
        later.created_at = LATER.into();
        assert_eq!(normalize_event(&later).unwrap().unwrap().key, opened.key);
    }

    #[test]
    fn push_after_matches_reduced_head_without_commit_timestamps() {
        let (hook, _) = pair(
            "push",
            "PushEvent",
            json!({"ref":"refs/heads/main","before":"a".repeat(40),"after":"b".repeat(40),
                "head_commit":{"timestamp":"2000-01-01T00:00:00Z"},
                "commits":[{"id":"b".repeat(40),"message":"A new commit"}]}),
            json!({"repository_id":REPO_ID,"push_id":49052426883u64,
                "ref":"refs/heads/main","before":"a".repeat(40),"head":"b".repeat(40)}),
        );
        assert_eq!(hook.occurred_at, timestamp(TIME).unwrap());
        assert_eq!(
            hook.message["embeds"][0]["description"],
            "bbbbbbb A new commit"
        );
        let mut missing = webhook(json!({
            "ref":"refs/heads/main","before":"a".repeat(40),"after":"b".repeat(40),
            "head_commit":{"timestamp":TIME}
        }));
        missing["repository"]["pushed_at"] = Value::Null;
        assert!(normalize_webhook("push", &missing).is_err());
    }

    #[test]
    fn comments_and_submitted_reviews_match_across_sources() {
        for (hook_kind, feed_kind, object) in [
            ("issue_comment", "IssueCommentEvent", "issue"),
            (
                "pull_request_review_comment",
                "PullRequestReviewCommentEvent",
                "pull_request",
            ),
            ("discussion_comment", "DiscussionCommentEvent", "discussion"),
        ] {
            for action in ["created", "edited"] {
                let mut hook = json!({"action":action,"comment":{
                    "id":91,"body":"rich body","created_at":TIME,"updated_at":TIME
                }});
                hook[object] = json!({"number":258,"title":"rich title"});
                let mut feed = json!({"action":action,"comment":{"id":91}});
                feed[object] = json!({"number":258});
                pair(hook_kind, feed_kind, hook, feed);
            }
        }
        pair(
            "pull_request_review",
            "PullRequestReviewEvent",
            json!({"action":"submitted","pull_request":{"number":258,"title":"rich title"},
                "review":{"id":99,"submitted_at":TIME,"state":"approved","body":"LGTM"}}),
            json!({"action":"created","pull_request":{"number":258},"review":{"id":99}}),
        );
    }

    #[test]
    fn repeated_close_reopen_and_comment_edits_remain_distinct() {
        for (hook_kind, feed_kind, object) in [
            ("pull_request", "PullRequestEvent", "pull_request"),
            ("issues", "IssuesEvent", "issue"),
        ] {
            for action in ["closed", "reopened"] {
                let mut first = json!({"action":action});
                first[object] = json!({
                    "number":258,"merged":false,"closed_at":TIME,"updated_at":TIME
                });
                let (_, first_notification) =
                    pair(hook_kind, feed_kind, first.clone(), first.clone());
                first[object]["closed_at"] = json!(LATER);
                first[object]["updated_at"] = json!(LATER);
                let later = normalize_webhook(hook_kind, &webhook(first.clone()))
                    .unwrap()
                    .unwrap();
                let mut feed = event("next-transition", feed_kind, first);
                feed.created_at = LATER.into();
                assert_eq!(later.key, normalize_event(&feed).unwrap().unwrap().key);
                assert_ne!(later.key, first_notification.key);
            }
        }
        let mut edit = webhook(json!({"action":"edited","issue":{"number":258},
            "comment":{"id":91,"created_at":TIME,"updated_at":TIME}}));
        let first = normalize_webhook("issue_comment", &edit).unwrap().unwrap();
        edit["comment"]["updated_at"] = json!(LATER);
        assert_ne!(
            first.key,
            normalize_webhook("issue_comment", &edit)
                .unwrap()
                .unwrap()
                .key
        );
    }

    #[test]
    fn release_and_discussion_pairs_keep_resource_identity() {
        pair(
            "release",
            "ReleaseEvent",
            json!({"action":"published","release":{"id":14,"draft":false,"published_at":TIME,
                "name":"Release","tag_name":"v1","body":"Notes"}}),
            json!({"action":"published","release":{"id":14}}),
        );
        pair(
            "discussion",
            "DiscussionEvent",
            json!({"action":"created","discussion":{"number":15,"created_at":TIME,"title":"Topic"}}),
            json!({"action":"created","discussion":{"number":15}}),
        );
    }

    #[test]
    fn private_wrong_repository_and_uninteresting_actions_never_announce() {
        let mut hook = webhook(json!({"action":"opened","number":258,
            "pull_request":{"number":258,"created_at":TIME,"title":"secret"}}));
        hook["repository"]["private"] = json!(true);
        assert!(normalize_webhook("pull_request", &hook).unwrap().is_none());
        hook["repository"]["private"] = json!(false);
        hook["repository"]["id"] = json!(123);
        assert!(normalize_webhook("pull_request", &hook).unwrap().is_none());
        hook["repository"]["id"] = json!(REPO_ID);
        hook["repository"]["full_name"] = json!("another/repository");
        assert!(normalize_webhook("pull_request", &hook).unwrap().is_none());
        let mut private = event("1", "PullRequestEvent", json!({}));
        private.public = false;
        assert!(normalize_event(&private).unwrap().is_none());
        for action in ["synchronize", "labeled", "assigned", "edited"] {
            assert!(
                normalize_webhook("pull_request", &webhook(json!({"action":action})))
                    .unwrap()
                    .is_none()
            );
        }
        assert!(normalize_webhook("ping", &json!({})).unwrap().is_none());
        assert!(
            normalize_event(&event("1", "WatchEvent", json!({})))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn malformed_supported_payloads_are_errors_without_panics() {
        for payload in [
            json!({}),
            json!({"action":"opened","pull_request":{}}),
            json!({"action":"opened","number":1,"pull_request":{"created_at":"not-a-date"}}),
            json!({"action":"opened","number":1,"pull_request":{"created_at":12}}),
            json!({"action":"opened","number":1,"pull_request":{"created_at":"x".repeat(100)}}),
            json!({"action":"closed","number":1,"pull_request":{"closed_at":TIME}}),
        ] {
            assert!(normalize_webhook("pull_request", &webhook(payload)).is_err());
        }
        let mut hook = webhook(json!({}));
        hook["repository"]["private"] = Value::Null;
        assert!(normalize_webhook("pull_request", &hook).is_err());
        assert!(
            normalize_webhook(
                "issue_comment",
                &webhook(json!({
                    "action":"created","issue":{"number":1},"comment":{"created_at":TIME}
                }))
            )
            .is_err()
        );
        assert!(
            normalize_webhook(
                "push",
                &webhook(json!({
                    "ref":"refs/heads/main","before":"bad","after":"b".repeat(40)
                }))
            )
            .is_err()
        );
        assert!(
            normalize_event(&event(
                "1",
                "PullRequestEvent",
                json!({
                    "action":"merged","number":1,"pull_request":{"merged_at":"invalid"}
                })
            ))
            .is_err()
        );
    }

    #[test]
    fn rfc3339_offsets_and_durable_markers_are_canonical() {
        assert_eq!(
            timestamp(TIME).unwrap(),
            timestamp("2026-11-05T03:42:55+01:00").unwrap()
        );
        let mut first = message("one", "body", WEB, None);
        stamp_message(&mut first, "abc");
        let mut changed = message("two", "different body", WEB, Some("999"));
        stamp_message(&mut changed, "abc");
        assert!(first["nonce"].as_str().unwrap().len() <= 25);
        assert_eq!(first["nonce"], changed["nonce"]);
        assert_eq!(first["embeds"][0]["footer"], changed["embeds"][0]["footer"]);
        let once = first.clone();
        stamp_message(&mut first, "abc");
        assert_eq!(first, once);
        assert_eq!(first["enforce_nonce"], true);
    }

    #[test]
    fn timestamp_free_webhooks_keep_coarse_cross_source_identity() {
        for (hook_kind, feed_kind, payload) in [
            (
                "create",
                "CreateEvent",
                json!({"ref":"topic","ref_type":"branch"}),
            ),
            (
                "delete",
                "DeleteEvent",
                json!({"ref":"topic","ref_type":"branch"}),
            ),
            (
                "gollum",
                "GollumEvent",
                json!({"pages":[{"page_name":"Home","sha":"a".repeat(40)}]}),
            ),
            ("public", "PublicEvent", json!({})),
            (
                "issue_comment",
                "IssueCommentEvent",
                json!({
                    "action":"deleted","issue":{"number":1},"comment":{"id":2,"updated_at":TIME}
                }),
            ),
            (
                "pull_request_review",
                "PullRequestReviewEvent",
                json!({
                    "action":"dismissed","pull_request":{"number":1},"review":{"id":2,"submitted_at":TIME}
                }),
            ),
        ] {
            let hook = normalize_webhook(hook_kind, &webhook(payload.clone()))
                .unwrap()
                .unwrap();
            let mut feed = event("1", feed_kind, payload);
            let old = normalize_event(&feed).unwrap().unwrap();
            assert_eq!(hook.key, old.key);
            assert_eq!(hook.message["nonce"], old.message["nonce"]);
            feed.created_at = LATER.into();
            assert_eq!(old.key, normalize_event(&feed).unwrap().unwrap().key);
        }
        assert!(normalize_webhook("pull_request_review", &webhook(json!({
            "action":"edited","pull_request":{"number":1},"review":{"id":2,"submitted_at":TIME}
        }))).unwrap().is_none());
    }

    #[test]
    fn reduced_merge_and_push_payloads_have_stable_links() {
        let merged = render(&event(
            "10",
            "PullRequestEvent",
            json!({"action":"merged","number":258,"pull_request":{"number":258}}),
        ))
        .unwrap();
        assert_eq!(merged["embeds"][0]["title"], "alice merged PR #258");
        assert_eq!(merged["embeds"][0]["url"], format!("{WEB}/pull/258"));
        let push = render(&event(
            "2",
            "PushEvent",
            json!({"ref":"refs/heads/main","before":"a".repeat(40),"head":"b".repeat(40)}),
        ))
        .unwrap();
        assert!(
            push["embeds"][0]["url"]
                .as_str()
                .unwrap()
                .contains("/compare/")
        );
        assert_eq!(push["embeds"][0]["description"], "");
    }

    #[test]
    fn bounds_and_mentions_are_safe() {
        let notification = message(
            &"😀".repeat(300),
            &format!("@everyone <@123> {}", "😀".repeat(5000)),
            WEB,
            Some("123"),
        );
        assert!(
            notification["embeds"][0]["title"]
                .as_str()
                .unwrap()
                .encode_utf16()
                .count()
                <= 256
        );
        assert!(
            notification["embeds"][0]["description"]
                .as_str()
                .unwrap()
                .encode_utf16()
                .count()
                <= 4096
        );
        assert_eq!(notification["allowed_mentions"]["parse"], json!([]));
        assert_eq!(notification["allowed_mentions"]["users"], json!([]));
        assert_eq!(notification["allowed_mentions"]["roles"], json!([]));
        assert_eq!(notification["allowed_mentions"]["replied_user"], false);
        assert_eq!(notification["nonce"], &marker("123")[5..29]);
        assert_eq!(notification["enforce_nonce"], true);
    }

    #[test]
    fn unknown_private_and_incomplete_events_are_safe() {
        assert!(render(&event("1", "WatchEvent", json!({}))).is_none());
        assert!(render(&event("1", "FutureEvent", json!({}))).is_none());
        let mut private = event("2", "IssuesEvent", json!({}));
        private.public = false;
        assert!(render(&private).is_none());
        let incomplete = render(&event("3", "PullRequestReviewCommentEvent", json!({}))).unwrap();
        assert_eq!(incomplete["embeds"][0]["url"], format!("{WEB}/pulls"));
        let closed = render(&event(
            "4",
            "PullRequestEvent",
            json!({"action":"closed","number":1,"pull_request":{"merged":true}}),
        ))
        .unwrap();
        assert_eq!(closed["embeds"][0]["title"], "alice merged PR #1");
    }
}
