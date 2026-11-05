mod db;
mod events;
mod http;
mod runtime;
mod webhook;

use anyhow::{Context, Result, bail, ensure};
use events::{Event, REPO};
use http::Api;
use reqwest::Method;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::PathBuf;

const CHANNEL_NAME: &str = "github-notifications";
const VIEW: u64 = 1 << 10;
const SEND: u64 = 1 << 11;
const EMBED: u64 = 1 << 14;
const HISTORY: u64 = 1 << 16;
const ADMIN: u64 = 1 << 3;
const BOT_PERMISSIONS: u64 = VIEW | SEND | EMBED | HISTORY;

fn main() {
    if let Err(error) = execute() {
        eprintln!("sekka-discord: {error:#}");
        std::process::exit(1);
    }
}

fn execute() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("--help");
    ensure!(args.len() <= 1, "Expected exactly one command; use --help");
    match command {
        "--help" | "-h" | "help" => {
            println!(
                "sekka-discord <run|setup|check|smoke|preview>\n\nReports public bitemyapp/sekka GitHub webhooks through Discord REST.\n\nDISCORD_GUILD_ID            Required except for preview\nDISCORD_CHANNEL_ID          Required for run; otherwise discover github-notifications\nDISCORD_TOKEN_FILE          Default: ~/.config/sekka-discord/bot-token\nGITHUB_WEBHOOK_SECRET_FILE  Default: ~/.config/sekka-discord/webhook-secret\nSEKKA_STATE_DIR             Default: ~/.local/state/sekka-discord\nSEKKA_LISTEN_ADDR           Default: 127.0.0.1:8787 (loopback only)\nSEKKA_ACTIVATED_AT          Initial RFC3339 cutoff; persisted once in SQLite\nGITHUB_TOKEN_FILE           Optional API token file (public data only)\n\nrun binds the signed webhook listener before startup Events API catch-up.\nsetup creates the channel if missing; run never creates channels.\npreview reads public GitHub events without Discord credentials or writes."
            );
            Ok(())
        }
        "preview" => preview(),
        "run" => runtime::run(&Config::load()?),
        "setup" | "check" | "smoke" => {
            let config = Config::load()?;
            let discord = Api::new(false, Some(&read_token(&config.token_file)?))?;
            let identity = discord.get("/users/@me")?;
            ensure!(
                identity["bot"].as_bool() == Some(true),
                "Discord credentials must identify a bot account"
            );
            let bot_id = snowflake(&identity["id"], "Discord bot ID")?;
            let channel = resolve_channel(&discord, &config, &bot_id, command == "setup")?;
            validate_channel(&channel, &config.guild)?;
            let channel_id = snowflake(&channel["id"], "Discord channel ID")?;
            validate_permissions(&discord, &config.guild, &bot_id, &channel)?;
            match command {
                "smoke" => smoke(&discord, &channel_id, &bot_id),
                _ => {
                    let channel_name = channel["name"].as_str().unwrap_or("unknown");
                    println!(
                        "Discord bot {bot_id} validated in guild {}, channel {channel_id} ({channel_name}).",
                        config.guild
                    );
                    println!("DISCORD_CHANNEL_ID={channel_id}");
                    Ok(())
                }
            }
        }
        _ => bail!("Unknown command; use sekka-discord --help"),
    }
}

struct Config {
    guild: String,
    channel: Option<String>,
    token_file: PathBuf,
    state_dir: PathBuf,
}

impl Config {
    fn load() -> Result<Self> {
        let guild =
            env::var("DISCORD_GUILD_ID").context("Set DISCORD_GUILD_ID to the target server ID")?;
        validate_id(&guild, "DISCORD_GUILD_ID")?;
        let channel = env::var("DISCORD_CHANNEL_ID")
            .ok()
            .filter(|s| !s.is_empty());
        if let Some(channel) = &channel {
            validate_id(channel, "DISCORD_CHANNEL_ID")?;
        }
        Ok(Self {
            guild,
            channel,
            token_file: configured_path("DISCORD_TOKEN_FILE", ".config/sekka-discord/bot-token")?,
            state_dir: configured_path("SEKKA_STATE_DIR", ".local/state/sekka-discord")?,
        })
    }
}

fn configured_path(variable: &str, suffix: &str) -> Result<PathBuf> {
    match env::var_os(variable) {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        _ => Ok(
            PathBuf::from(env::var_os("HOME").context("HOME is required for default paths")?)
                .join(suffix),
        ),
    }
}

fn read_token(path: &std::path::Path) -> Result<String> {
    let token = fs::read_to_string(path)
        .context("Reading token file (check configured file and permissions)")?;
    let token = token.trim();
    ensure!(!token.is_empty(), "Token file is empty");
    Ok(token.to_owned())
}

fn github() -> Result<Api> {
    let token = env::var_os("GITHUB_TOKEN_FILE")
        .filter(|p| !p.is_empty())
        .map(|path| read_token(&PathBuf::from(path)))
        .transpose()?;
    Api::new(true, token.as_deref())
}

fn validate_id(value: &str, name: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.bytes().all(|b| b.is_ascii_digit())
            && value.parse::<u64>().is_ok_and(|n| n > 0),
        "{name} must be a Discord numeric ID"
    );
    Ok(())
}

fn snowflake(value: &Value, name: &str) -> Result<String> {
    let value = value.as_str().with_context(|| format!("Missing {name}"))?;
    validate_id(value, name)?;
    Ok(value.to_owned())
}

fn validate_channel(channel: &Value, guild: &str) -> Result<()> {
    ensure!(
        channel["guild_id"].as_str() == Some(guild),
        "Discord channel does not belong to DISCORD_GUILD_ID"
    );
    ensure!(
        channel["type"].as_u64() == Some(0),
        "Discord channel must be a guild text channel"
    );
    Ok(())
}

fn resolve_channel(api: &Api, config: &Config, bot_id: &str, setup: bool) -> Result<Value> {
    if let Some(id) = &config.channel {
        return api.get(&format!("/channels/{id}"));
    }
    let channels = api.get(&format!("/guilds/{}/channels", config.guild))?;
    let channels = channels
        .as_array()
        .context("Invalid Discord channel list")?;
    let named: Vec<&Value> = channels
        .iter()
        .filter(|channel| channel["name"].as_str() == Some(CHANNEL_NAME))
        .collect();
    ensure!(
        named.len() <= 1,
        "Multiple channels named {CHANNEL_NAME}; set DISCORD_CHANNEL_ID explicitly rather than guessing"
    );
    if let Some(channel) = named.first() {
        return Ok((*channel).clone());
    }
    ensure!(
        setup,
        "No {CHANNEL_NAME} channel exists; run sekka-discord setup with Manage Channels permission first"
    );
    api.request(Method::POST, &format!("/guilds/{}/channels", config.guild), Some(&json!({
        "name": CHANNEL_NAME,
        "type": 0,
        "topic": "Public GitHub activity for bitemyapp/sekka. Events can be delayed; not a complete audit log.",
        "permission_overwrites": [
            {"id": config.guild, "type": 0, "allow": (VIEW | HISTORY).to_string(), "deny": "0"},
            {"id": bot_id, "type": 1, "allow": BOT_PERMISSIONS.to_string(), "deny": "0"}
        ]
    })))
}

fn permissions(value: &Value) -> Result<u64> {
    value
        .as_str()
        .context("Discord permission bitset missing")?
        .parse()
        .context("Invalid Discord permission bitset")
}

fn overwrite(base: u64, value: &Value) -> Result<u64> {
    Ok((base & !permissions(&value["deny"])?) | permissions(&value["allow"])?)
}

fn validate_permissions(api: &Api, guild: &str, bot_id: &str, channel: &Value) -> Result<()> {
    let roles = api.get(&format!("/guilds/{guild}/roles"))?;
    let roles = roles.as_array().context("Invalid guild roles response")?;
    let member = api.get(&format!("/guilds/{guild}/members/{bot_id}"))?;
    let member_roles = member["roles"]
        .as_array()
        .context("Invalid bot membership response")?;
    let everyone = roles
        .iter()
        .find(|role| role["id"].as_str() == Some(guild))
        .context("Guild everyone role not found")?;
    let everyone_base = permissions(&everyone["permissions"])?;
    let overwrites = channel["permission_overwrites"]
        .as_array()
        .context("Missing channel permissions")?;
    let everyone_override = overwrites
        .iter()
        .find(|o| o["type"].as_u64() == Some(0) && o["id"].as_str() == Some(guild));
    let community = match everyone_override {
        Some(value) => overwrite(everyone_base, value)?,
        None => everyone_base,
    };
    ensure!(
        community & (VIEW | HISTORY) == VIEW | HISTORY || everyone_base & ADMIN != 0,
        "Channel must grant @everyone View Channel and Read Message History; adjust this channel's permissions before using it"
    );
    let mut bot = everyone_base;
    for role in roles {
        if member_roles.iter().any(|id| id == &role["id"]) {
            bot |= permissions(&role["permissions"])?;
        }
    }
    if bot & ADMIN != 0 {
        return Ok(());
    }
    if let Some(value) = everyone_override {
        bot = overwrite(bot, value)?;
    }
    let mut deny = 0;
    let mut allow = 0;
    for value in overwrites {
        if value["type"].as_u64() == Some(0) && member_roles.iter().any(|id| id == &value["id"]) {
            deny |= permissions(&value["deny"])?;
            allow |= permissions(&value["allow"])?;
        }
    }
    bot = (bot & !deny) | allow;
    if let Some(value) = overwrites
        .iter()
        .find(|o| o["type"].as_u64() == Some(1) && o["id"].as_str() == Some(bot_id))
    {
        bot = overwrite(bot, value)?;
    }
    ensure!(
        bot & BOT_PERMISSIONS == BOT_PERMISSIONS,
        "Bot needs View Channel, Send Messages, Embed Links and Read Message History in the notifications channel"
    );
    Ok(())
}

fn smoke(api: &Api, channel: &str, bot: &str) -> Result<()> {
    let body = events::message(
        "Sekka bot setup verification",
        "SETUP VERIFICATION ONLY — this is not a GitHub event. Discord delivery and message read-back are being checked.",
        "https://github.com/bitemyapp/sekka",
        None,
    );
    let sent = api.request(
        Method::POST,
        &format!("/channels/{channel}/messages"),
        Some(&body),
    )?;
    let id = snowflake(&sent["id"], "verification message ID")?;
    let received = api.get(&format!("/channels/{channel}/messages/{id}"))?;
    ensure!(
        received["channel_id"].as_str() == Some(channel)
            && received["author"]["id"].as_str() == Some(bot)
            && received["embeds"][0]["title"] == body["embeds"][0]["title"]
            && received["embeds"][0]["description"] == body["embeds"][0]["description"],
        "Setup verification message did not read back correctly"
    );
    println!("Setup verification delivered and read back: message {id}, channel {channel}");
    Ok(())
}

fn fetch_events(api: &Api) -> Result<Vec<Event>> {
    // Authentication may raise the rate limit, but must never expand visibility.
    let repo = api.get(&format!("/repos/{REPO}"))?;
    ensure!(
        repo["private"].as_bool() == Some(false) && repo["full_name"].as_str() == Some(REPO),
        "Refusing to report a repository that is not confirmed public"
    );
    let mut events = Vec::with_capacity(300);
    for page in 1..=3 {
        let value = api.get(&format!("/repos/{REPO}/events?per_page=100&page={page}"))?;
        let batch: Vec<Event> =
            serde_json::from_value(value).context("Invalid GitHub Events response")?;
        ensure!(
            batch.iter().all(|event| !event.id.is_empty()),
            "GitHub event has no ID; refusing unsafe checkpoint"
        );
        let count = batch.len();
        events.extend(batch);
        if count < 100 {
            break;
        }
    }
    Ok(events)
}

fn preview() -> Result<()> {
    let api = github()?;
    let events = fetch_events(&api)?;
    let mut displayed = 0;
    for event in events.iter().take(20) {
        if let Some(message) = events::render(event) {
            println!("{}", serde_json::to_string_pretty(&message)?);
            displayed += 1;
        }
    }
    eprintln!(
        "Read-only preview: {displayed} notifications from up to 20 recent events ({} events fetched); no Discord requests or state writes",
        events.len()
    );
    Ok(())
}
