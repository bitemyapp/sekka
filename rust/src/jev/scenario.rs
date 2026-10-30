use super::*;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    id: String,
    description: String,
    max_calls: u64,
    max_input_tokens: u64,
    max_seconds: u64,
    min_probability: f64,
    min_box_confidence: f64,
    steps: Vec<Step>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    id: String,
    precondition: String,
    action: Action,
    expected: String,
    timeout_seconds: u64,
    #[serde(default)]
    expect_exit: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    Observe,
    Click {
        target: String,
    },
    Fill {
        target: String,
        text: String,
        #[serde(default = "default_select_all")]
        select_all: Vec<String>,
    },
    Key {
        keys: Vec<String>,
    },
    Type {
        text: String,
    },
}

fn default_select_all() -> Vec<String> {
    vec!["ctrl".into(), "a".into()]
}

fn valid_keys(keys: &[String]) -> bool {
    !keys.is_empty()
        && keys.len() <= 4
        && keys.iter().all(|k| {
            !k.is_empty()
                && k.len() < 32
                && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        })
}

fn validate(s: &Scenario) -> Result<()> {
    ensure!(
        !s.id.is_empty() && !s.steps.is_empty() && s.steps.len() <= 500,
        "Scenario needs an ID and 1..500 steps"
    );
    ensure!(
        (0.9..=1.).contains(&s.min_probability) && (0.5..=1.).contains(&s.min_box_confidence),
        "Invalid decision thresholds"
    );
    ensure!(
        (1..=7200).contains(&s.max_seconds),
        "Invalid scenario duration"
    );
    let mut ids = std::collections::HashSet::new();
    for step in &s.steps {
        ensure!(
            ids.insert(&step.id) && !step.id.is_empty(),
            "Duplicate/empty step ID"
        );
        ensure!(
            !step.precondition.is_empty() && !step.expected.is_empty(),
            "Every step needs a precondition and postcondition"
        );
        ensure!(
            (1..=1800).contains(&step.timeout_seconds),
            "Invalid step timeout"
        );
        match &step.action {
            Action::Type { text } | Action::Fill { text, .. } => ensure!(
                text.len() <= 4096 && text.chars().all(|c| (' '..='~').contains(&c)),
                "Typing supports printable ASCII in the US layout only"
            ),
            Action::Key { keys } => ensure!(valid_keys(keys), "Invalid QEMU key chord"),
            _ => {}
        }
        if let Action::Fill { select_all, .. } = &step.action {
            ensure!(valid_keys(select_all), "Invalid select-all key chord");
        }
    }
    Ok(())
}

fn noul(instructions: &str) -> Value {
    json!({"type":"noul","instructions":format!("In `screen`, {instructions} Answer from visible pixels only.")})
}

fn check_deadline(deadline: Instant) -> Result<()> {
    crate::support::check_interrupt()?;
    ensure!(Instant::now() < deadline, "Scenario deadline exceeded");
    Ok(())
}

fn target_checks(response: &Value, threshold: f64) -> Result<()> {
    ensure!(
        api::probability(response, "present")? >= threshold,
        "Target presence uncertain; no input sent"
    );
    ensure!(
        api::probability(response, "obscured")? <= 1. - threshold,
        "Target obscured check uncertain; no input sent"
    );
    api::unique_match(response, threshold)
}

fn observe(
    run: &Path,
    out: &Path,
    client: &mut api::Client,
    questions: Value,
    deadline: Instant,
) -> Result<(PathBuf, Value, Instant)> {
    check_deadline(deadline)?;
    let image = machine::capture(run)?;
    let captured = Instant::now();
    let response = client.ask(&image, questions)?;
    event(
        out,
        json!({"kind":"observation","screenshot":image,"sha256":hash(&image)?,"response":response}),
    )?;
    check_deadline(deadline)?;
    Ok((image, response, captured))
}

fn execute(
    run: &Path,
    out: &Path,
    client: &mut api::Client,
    s: &Scenario,
    step: &Step,
    deadline: Instant,
) -> Result<Value> {
    let target = match &step.action {
        Action::Click { target } | Action::Fill { target, .. } => Some(target),
        _ => None,
    };
    let mut questions = json!({"ready":noul(&step.precondition)});
    if let Some(target) = target {
        questions["present"] = noul(&format!("Is the control '{target}' visible?"));
        questions["disabled"] = noul(&format!(
            "Is the control '{target}' grayed out or marked unavailable?"
        ));
        questions["obscured"] = noul(&format!(
            "Is the control '{target}' covered by another window or panel?"
        ));
        questions["matches"] = json!({"type":"choice","instructions":format!("How many controls matching '{target}' are visible in `screen`?"),"criteria":{"none":"No matching control is visible.","one":"Exactly one matching control is visible.","several":"Several matching controls are visible."}});
        questions["target"] = json!({"type":"box","instructions":format!("The clickable control '{target}' in `screen`. Return the tight bounding box of the control itself.")});
    }
    let wait_until = std::cmp::min(
        deadline,
        Instant::now() + Duration::from_secs(step.timeout_seconds),
    );
    let (before, response, captured) = loop {
        let observation = observe(run, out, client, questions.clone(), wait_until)?;
        if api::probability(&observation.1, "ready")? >= s.min_probability {
            break observation;
        }
        crate::support::pause(Duration::from_secs(2))?;
    };
    ensure!(
        captured.elapsed() < Duration::from_secs(5),
        "Frame older than 5 seconds; no input sent"
    );
    let (w, h) = api::dimensions(&before)?;
    let point = if target.is_some() {
        target_checks(&response, s.min_probability)?;
        let direct = (|| {
            ensure!(
                api::probability(&response, "disabled")? <= 1. - s.min_probability,
                "Enabled state uncertain"
            );
            api::center(&response, "target", s.min_box_confidence, w, h)
        })();
        match direct {
            Ok(point) => Some(point),
            Err(_) => {
                let crop = out.join(format!("control-crop-{}.png", now()));
                let (x, y, cw, ch) = api::crop_control(&before, &response, &crop)?;
                event(
                    out,
                    json!({"kind":"locator-crop","original":before,"crop":crop,"region":[x,y,cw,ch],"transform":"unscaled crop; add x/y to local pixel coordinates"}),
                )?;
                let mut local_questions = questions.clone();
                local_questions.as_object_mut().unwrap().remove("ready");
                let local = client.ask(&crop, local_questions)?;
                target_checks(&local, s.min_probability)?;
                ensure!(
                    api::probability(&local, "disabled")? <= 1. - s.min_probability,
                    "Target disabled check uncertain after crop; no input sent"
                );
                let (lx, ly) = api::center(&local, "target", s.min_box_confidence, cw, ch)?;
                Some((x + lx, y + ly))
            }
        }
    } else {
        None
    };
    ensure!(
        captured.elapsed() < Duration::from_secs(5),
        "Frame older than 5 seconds after localization; no input sent"
    );
    event(
        out,
        json!({"kind":"input-intent","id":step.id,"action":step.action,"point":point,"screenshot":before,"precondition":step.precondition}),
    )?;
    match &step.action {
        Action::Observe => {}
        Action::Click { .. } => {
            let (x, y) = point.unwrap();
            machine::click(run, x, y, w, h)?;
        }
        Action::Fill {
            text, select_all, ..
        } => {
            let (x, y) = point.unwrap();
            machine::click(run, x, y, w, h)?;
            crate::support::pause(Duration::from_millis(150))?;
            let mut qmp = machine::connect(run)?;
            crate::gui_input::key(
                &mut qmp,
                &select_all.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
            crate::gui_input::type_text(&mut qmp, text)?;
        }
        Action::Key { keys } => crate::gui_input::key(
            &mut machine::connect(run)?,
            &keys.iter().map(String::as_str).collect::<Vec<_>>(),
        )?,
        Action::Type { text } => crate::gui_input::type_text(&mut machine::connect(run)?, text)?,
    }
    event(out, json!({"kind":"input-completed","id":step.id}))?;
    if step.expect_exit {
        let until = std::cmp::min(
            deadline,
            Instant::now() + Duration::from_secs(step.timeout_seconds),
        );
        while !machine::stopped(run)? {
            check_deadline(until)?;
            crate::support::pause(Duration::from_millis(500))?;
        }
        return Ok(
            json!({"id":step.id,"status":"observed","qualification_status":"not-run","precondition":step.precondition,"action":step.action,"expected":step.expected,"before":before,"observed":"QEMU process exited after UI input","assertion":"process-identity-checked","review_required":true}),
        );
    }
    // Never repeat an action because its postcondition was uncertain.
    // Require two fresh observations; this is still a model claim, not certification.
    let wait_until = std::cmp::min(
        deadline,
        Instant::now() + Duration::from_secs(step.timeout_seconds),
    );
    let mut consecutive = 0;
    loop {
        crate::support::pause(Duration::from_secs(2))?;
        let (after, response, _) = observe(
            run,
            out,
            client,
            json!({"expected":noul(&step.expected)}),
            wait_until,
        )?;
        let probability = api::probability(&response, "expected")?;
        consecutive = if probability >= s.min_probability {
            consecutive + 1
        } else {
            0
        };
        if consecutive == 2 {
            return Ok(
                json!({"id":step.id,"status":"observed","qualification_status":"not-run","precondition":step.precondition,"action":step.action,"expected":step.expected,"before":before,"after":after,"probability":probability,"review_required":true}),
            );
        }
    }
}

pub fn run(run: &Path, scenario: &Path, dotenv: Option<&Path>) -> Result<()> {
    let run = run.canonicalize()?;
    let _lock = machine::lock(&run)?;
    let plan: Scenario = serde_json::from_value(read(scenario)?)?;
    validate(&plan)?;
    let out = private_dir(&run.join(format!("scenario-{}-{}", now(), std::process::id())))?;
    save(&out.join("scenario.json"), &serde_json::to_value(&plan)?)?;
    save(
        &out.join("driver.json"),
        &json!({"harness_sha256":hash(&std::env::current_exe()?)?,"started_ms":now(),"phase":read(&run.join("active.json"))?}),
    )?;
    let mut client = api::Client::new(&out, dotenv, plan.max_calls, plan.max_input_tokens)?;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(plan.max_seconds);
    let mut ledger:Vec<Value>=plan.steps.iter().map(|s|json!({"id":s.id,"status":"not-run","qualification_status":"not-run","expected":s.expected})).collect();
    let mut failure = None;
    for (index, step) in plan.steps.iter().enumerate() {
        event(&out, json!({"kind":"step-start","id":step.id}))?;
        match execute(&run, &out, &mut client, &plan, step, deadline) {
            Ok(result) => {
                println!("{}: observed (review required)", step.id);
                event(&out, json!({"kind":"step-observed","result":result}))?;
                ledger[index] = result;
            }
            Err(error) => {
                let error = format!("{error:#}");
                ledger[index] = json!({"id":step.id,"status":"blocked","qualification_status":"not-run","expected":step.expected,"error":error});
                event(&out, json!({"kind":"step-blocked","result":ledger[index]}))?;
                failure = Some(error);
                break;
            }
        }
    }
    save(
        &out.join("report.json"),
        &json!({"scenario":plan.id,"status":if failure.is_some(){"incomplete"}else{"completed"},"qualification":"not certified","publication_enabled":false,"elapsed_ms":start.elapsed().as_millis(),"metrics":client.summary(),"ledger":ledger,"run_manifest":read(&run.join("manifest.json"))?,"phase":read(&run.join("active.json"))?}),
    )?;
    println!("Evidence: {}", out.display());
    if let Some(error) = failure {
        bail!("Scenario stopped: {error}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fill_keeps_desktop_specific_select_all_chord() {
        let value = json!({"kind":"fill","target":"name","text":"test"});
        let Action::Fill { select_all, .. } =
            serde_json::from_value::<Action>(value.clone()).unwrap()
        else {
            panic!("fill")
        };
        assert_eq!(select_all, ["ctrl", "a"]);
        let mut native = value;
        native["select_all"] = json!(["meta_l", "a"]);
        let Action::Fill { select_all, .. } = serde_json::from_value::<Action>(native).unwrap()
        else {
            panic!("fill")
        };
        assert_eq!(select_all, ["meta_l", "a"]);
        assert!(valid_keys(&select_all));
        assert!(!valid_keys(&[]));
        assert!(!valid_keys(&["ctrl+a".into()]));
    }
    #[test]
    fn invalid_scenarios_are_rejected_before_input() {
        let mut s = Scenario {
            id: "example".into(),
            description: "test".into(),
            max_calls: 10,
            max_input_tokens: 655360,
            max_seconds: 10,
            min_probability: 0.95,
            min_box_confidence: 0.8,
            steps: vec![Step {
                id: "one".into(),
                precondition: "Desktop is visible".into(),
                action: Action::Type {
                    text: "safe".into(),
                },
                expected: "Text is visible".into(),
                timeout_seconds: 10,
                expect_exit: false,
            }],
        };
        assert!(validate(&s).is_ok());
        s.steps[0].action = Action::Type {
            text: "not\nprintable".into(),
        };
        assert!(validate(&s).is_err());
        s.steps[0].action = Action::Observe;
        s.min_probability = 0.1;
        assert!(validate(&s).is_err());
        assert!(
            serde_json::from_value::<Action>(json!({"kind":"shell","command":"evil"})).is_err()
        );
    }
}
