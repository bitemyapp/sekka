//! `tasuke ask`: one request to the agent from a terminal or a script. The
//! answer streams to stdout; tool steps and approvals go to stderr (or, with
//! `--json`, every event is a JSON line on stdout). The evaluation VM runs
//! it as root with `--approve all`, so the scenarios grade the product's
//! own agent.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use tasuke_agent::{
    Approval, Coords, DataDir, Decision, Event, FinishReason, Options, Session, SessionId, Skills, StepBudget, Tokens,
    ToolCall, ToolStatus,
};

const USAGE: &str = "\
usage: tasuke ask [options] REQUEST…   (REQUEST - reads it from stdin)

  --approve ask|all|never  who decides what needs approval (default: ask on a
                           terminal, never otherwise)
  --no-act                 only inspect and explain
  --desktop                Desktop control: screenshots, mouse and keyboard
  --session ID             continue a conversation
  --json                   print every event as a JSON line
  --answer-file PATH       also write the final answer there
  --model ID               a downloaded model, or anthropic/ID or openai/ID for
                           a cloud model through a connected account
  --base-url URL --model NAME --api-key KEY
                           a server other than Tasuke's own
  --vision --coords pixel|norm1000|norm1|affine:XS,XO,YS,YO --marks --desktop-only
  --max-steps N --max-seconds N --context N --max-tokens N
  --param KEY=JSON         an extra request field (repeatable)
  --keep-reasoning --skills-dir DIR --data-dir DIR --inspect-dir DIR
  --user-prefix CMD --aizomectl CMD --person WHO --env-note TEXT
                           (for the evaluation VM)";

#[derive(Clone, Copy, PartialEq)]
enum Approve {
    Ask,
    All,
    Never,
}

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

pub fn run(args: &[String]) -> i32 {
    let args: &[String] = if args.first().is_some_and(|a| a == "ask") { &args[1..] } else { args };
    match ask(args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("tasuke ask: {e}");
            2
        }
    }
}

fn split(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(str::to_owned).collect()
}

fn ask(args: &[String]) -> Result<i32, String> {
    let mut o = Options::from_config();
    let mut approve = None;
    let mut json = false;
    let mut session_id = None;
    let mut answer_file: Option<PathBuf> = None;
    let mut text = Vec::new();
    let mut explicit_vision = false;
    let mut explicit_coords = false;
    let (mut base_url, mut model, mut api_key) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(0);
            }
            "--approve" => {
                approve = Some(match val(a)?.as_str() {
                    "ask" => Approve::Ask,
                    "all" => Approve::All,
                    "never" => Approve::Never,
                    v => return Err(format!("--approve {v}: say ask, all or never")),
                })
            }
            "--act" => o.act = true,
            "--no-act" => o.act = false,
            "--desktop" => o.desktop_control = true,
            "--session" => session_id = Some(SessionId::parse(&val(a)?).ok_or("not a session id")?),
            "--json" => json = true,
            "--answer-file" => answer_file = Some(PathBuf::from(val(a)?)),
            "--base-url" => base_url = Some(val(a)?),
            "--model" => model = Some(val(a)?),
            "--api-key" => api_key = Some(val(a)?),
            "--vision" => {
                o.vision = true;
                explicit_vision = true;
            }
            "--coords" => {
                let v = val(a)?;
                o.coords = Coords::parse(&v).ok_or_else(|| format!("--coords {v}: not a convention"))?;
                explicit_coords = true;
            }
            "--marks" => o.set_of_marks = true,
            "--desktop-only" => o.desktop_only = true,
            "--max-steps" => o.step_budget = StepBudget(val(a)?.parse().map_err(|_| "--max-steps takes a number")?),
            "--max-seconds" => {
                o.time_limit = Some(Duration::from_secs(val(a)?.parse().map_err(|_| "--max-seconds takes a number")?))
            }
            "--context" => o.context_fallback = Tokens(val(a)?.parse().map_err(|_| "--context takes a number")?),
            "--max-tokens" => o.max_output_tokens = Tokens(val(a)?.parse().map_err(|_| "--max-tokens takes a number")?),
            "--param" => {
                let v = val(a)?;
                let (k, j) = v.split_once('=').ok_or("--param takes KEY=JSON")?;
                let j: Value = serde_json::from_str(j).unwrap_or_else(|_| Value::String(j.to_owned()));
                o.request_params.insert(k.to_owned(), j);
            }
            "--keep-reasoning" => o.keep_reasoning = true,
            "--skills-dir" => o.skills = Skills::discover_in(&[PathBuf::from(val(a)?)]),
            "--data-dir" => o.data_dir = DataDir::new(val(a)?),
            "--inspect-dir" => o.inspect_dir = Some(PathBuf::from(val(a)?)),
            "--user-prefix" => o.user_command_prefix = split(&val(a)?),
            "--aizomectl" => o.aizomectl = split(&val(a)?),
            "--env-note" => o.environment_note = Some(val(a)?),
            "--person" => o.person = Some(val(a)?),
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n{USAGE}")),
            _ => text.push(a.clone()),
        }
    }
    // "anthropic/claude-haiku-5-5" is a cloud model; a bare id is a local one.
    match model.as_deref().map(|m| (m, tasuke_models::CloudModelRef::parse(m))) {
        Some((_, Some(r))) => o.use_cloud(r.provider, &r.id),
        Some((m, None)) if o.provider.is_some() => o.use_local(m),
        Some((m, None)) => o.model = m.to_owned(),
        None => {}
    }
    if let Some(u) = base_url {
        o.base_url = u;
    }
    if let Some(k) = api_key {
        o.api_key = Some(k);
    }
    let mut request = text.join(" ");
    if request == "-" || (request.is_empty() && !std::io::stdin().is_terminal()) {
        request.clear();
        std::io::stdin().lock().read_to_string_lossy(&mut request);
    }
    let request = request.trim().to_owned();
    if request.is_empty() {
        return Err(format!("what should Tasuke do?\n{USAGE}"));
    }
    if o.model.is_empty() {
        return Err("no model is chosen: run `tasuke recommend`, then `tasuke use ID` (or pass --model)".into());
    }
    apply_catalog(&mut o, explicit_vision, explicit_coords);
    let approve = approve.unwrap_or(if std::io::stdin().is_terminal() { Approve::Ask } else { Approve::Never });

    let session = match &session_id {
        Some(id) => Session::load(&o.data_dir, id).map_err(|e| format!("conversation {id}: {e}"))?,
        None => Session::create(&o.data_dir).map_err(|e| format!("{}: {e}", o.data_dir.path().display()))?,
    };
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t);
    }
    let started = Instant::now();
    let turn = tasuke_agent::Agent::start(&session, &request, o);
    let mut answer = String::new();
    let mut calls: Vec<(String, ToolCall)> = Vec::new();
    let mut finished = None;
    let mut stdout = std::io::stdout().lock();
    let mut at_line_start = true;
    loop {
        if INTERRUPTED.swap(false, Ordering::SeqCst) {
            eprintln!("\n(stopping)");
            turn.cancel();
        }
        let event = match turn.events().recv_timeout(Duration::from_millis(100)) {
            Ok(e) => e,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        };
        if json {
            let _ = writeln!(stdout, "{}", serde_json::to_string(&event).unwrap_or_default());
            let _ = stdout.flush();
        }
        match &event {
            Event::TextDelta { text } => {
                answer.push_str(text);
                if !json {
                    let _ = write!(stdout, "{text}");
                    let _ = stdout.flush();
                    at_line_start = text.ends_with('\n');
                }
            }
            Event::ToolCall { id, call, needs } => {
                // Text before a tool call was narration; the answer is what comes after the last one.
                answer.clear();
                if !json && !at_line_start {
                    let _ = writeln!(stdout);
                    at_line_start = true;
                }
                calls.push((id.as_str().to_owned(), call.clone()));
                if !json {
                    eprintln!("› {}", call.summary());
                }
                if needs.asks() {
                    let decision = match approve {
                        Approve::All => Decision::Allow,
                        Approve::Never => Decision::Deny { note: Some("No one was there to approve it.".into()) },
                        Approve::Ask => ask_tty(call, needs),
                    };
                    turn.decide(id, decision);
                } else if let Approval::Refused { reason } = needs
                    && !json
                {
                    eprintln!("  ✕ not run: {reason}");
                }
            }
            Event::ToolOutput { id, output } if !json => {
                let mark = match output.status {
                    ToolStatus::Done if output.exit_code.is_none_or(|c| c == 0) => "✓",
                    ToolStatus::Done => "!",
                    _ => "✕",
                };
                let first = output.text.lines().find(|l| !l.trim().is_empty() && !l.starts_with("exit status"));
                let what = calls.iter().find(|(c, _)| c == id.as_str()).map(|(_, c)| c.tool_name()).unwrap_or("");
                eprintln!("  {mark} {what}{}", first.map(|l| format!(": {}", clip(l, 100))).unwrap_or_default());
            }
            Event::Notice { text, .. } if !json => eprintln!("· {text}"),
            Event::Error { text, .. } if !json => eprintln!("error: {text}"),
            Event::TurnFinished { reason, usage, decode_tps } => {
                finished = Some(*reason);
                if !json {
                    if !at_line_start {
                        let _ = writeln!(stdout);
                    }
                    let tps = decode_tps.map(|t| format!(" · {t:.0} tokens/s")).unwrap_or_default();
                    eprintln!(
                        "· {:.1} s · {} tool calls · {} prompt tokens ({} cached){tps} · conversation {}",
                        started.elapsed().as_secs_f32(),
                        usage.tool_calls,
                        usage.prompt_tokens,
                        usage.cached_tokens,
                        session.id()
                    );
                }
            }
            _ => {}
        }
    }
    turn.join();
    if let Some(p) = answer_file {
        std::fs::write(&p, answer.trim()).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(match finished {
        Some(FinishReason::Answered) => 0,
        Some(FinishReason::Cancelled) => 130,
        Some(FinishReason::Failed) | None => 1,
        Some(_) => 3,
    })
}

/// The catalog says how a model sees and points, and how it is sampled.
fn apply_catalog(o: &mut Options, explicit_vision: bool, explicit_coords: bool) {
    let catalog = tasuke_models::Catalog::load_default();
    let Some(entry) = catalog.get(&tasuke_models::ModelId::new(o.model.clone())) else { return };
    if !explicit_vision {
        o.vision = entry.vision.is_some();
    }
    if let Some(v) = entry.vision
        && !explicit_coords
    {
        o.coords = match v.coords {
            tasuke_models::Coords::Pixel => Coords::Pixel,
            tasuke_models::Coords::Norm1000 => Coords::Norm1000,
            tasuke_models::Coords::Norm1 => Coords::Norm1,
        };
    }
    o.keep_reasoning |= entry.keep_reasoning;
    for (k, v) in &entry.sampling {
        if let Ok(j) = serde_json::to_value(v) {
            o.request_params.entry(k.clone()).or_insert(j);
        }
    }
    if !entry.template_kwargs.is_empty() && !o.request_params.contains_key("chat_template_kwargs") {
        if let Ok(j) = serde_json::to_value(&entry.template_kwargs) {
            o.request_params.insert("chat_template_kwargs".into(), j);
        }
    }
}

fn clip(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

/// Ask on the terminal: the exact command, its purpose, why it asks.
fn ask_tty(call: &ToolCall, needs: &Approval) -> Decision {
    let reason = match needs {
        Approval::Ask { reason } | Approval::AskRoot { reason } => reason.as_str(),
        _ => "",
    };
    eprintln!();
    match call {
        ToolCall::RunCommand { command, purpose, .. } | ToolCall::RunAsRoot { command, purpose, .. } => {
            if !purpose.is_empty() {
                eprintln!("  Purpose: {purpose}");
            }
            for line in command.lines() {
                eprintln!("    {line}");
            }
        }
        ToolCall::WriteFile { diff, .. } | ToolCall::EditFile { diff, .. } => {
            for line in diff.lines().take(60) {
                eprintln!("    {line}");
            }
        }
        other => eprintln!("    {}", other.summary()),
    }
    eprintln!("  {reason}");
    let prompt = if matches!(needs, Approval::AskRoot { .. }) { "Run as administrator?" } else { "Run it?" };
    eprint!("  {prompt} [y/N, or say why not] ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return Decision::Deny { note: None };
    }
    let line = line.trim();
    match line.to_lowercase().as_str() {
        "y" | "yes" => Decision::Allow,
        "" | "n" | "no" => Decision::Deny { note: None },
        _ => Decision::Deny { note: Some(line.to_owned()) },
    }
}

trait ReadLossy {
    fn read_to_string_lossy(&mut self, out: &mut String);
}

impl<R: std::io::Read> ReadLossy for R {
    fn read_to_string_lossy(&mut self, out: &mut String) {
        let mut bytes = Vec::new();
        let _ = self.read_to_end(&mut bytes);
        out.push_str(&String::from_utf8_lossy(&bytes));
    }
}
