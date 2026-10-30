//! A tiny labeled screenshot corpus, independent of live VM qualification.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    description: String,
    min_probability: f64,
    min_confidence: f64,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    image: PathBuf,
    questions: Value,
    expected: std::collections::BTreeMap<String, Value>,
    click_rect: Option<[u32; 4]>,
}

pub fn run(path: &Path, out: &Path, dotenv: Option<&Path>) -> Result<()> {
    let path = path.canonicalize()?;
    let source = read(&path)?;
    let corpus: Corpus = serde_json::from_value(source.clone())?;
    ensure!(
        !corpus.cases.is_empty() && corpus.cases.len() <= 100,
        "Use 1..100 benchmark cases"
    );
    ensure!(
        (0.5..=1.).contains(&corpus.min_probability) && (0.0..=1.).contains(&corpus.min_confidence),
        "Invalid thresholds"
    );
    let out = private_dir(out)?;
    save(&out.join("corpus.json"), &source)?;
    let mut client = api::Client::new(&out, dotenv, 300, 10_000_000)?;
    let mut results = vec![];
    let mut correct = 0;
    let mut incorrect = 0;
    let mut abstained = 0;
    for (i, case) in corpus.cases.iter().enumerate() {
        let image = path.parent().unwrap().join(&case.image).canonicalize()?;
        let copy = out.join(format!("image-{i:03}.png"));
        fs::copy(&image, &copy)?;
        let response = client.ask(&copy, case.questions.clone())?;
        let mut judgments = vec![];
        for (id, expected) in &case.expected {
            let answer = &response["answers"][id];
            let (matches, accepted) = match expected {
                Value::Bool(truth) => {
                    let p = api::probability(&response, id)?;
                    (
                        (p >= 0.5) == *truth,
                        p >= corpus.min_probability || p <= 1. - corpus.min_probability,
                    )
                }
                Value::String(truth) => (
                    answer["choice"] == truth.as_str(),
                    answer["confidence"].as_f64().unwrap_or(0.) >= corpus.min_confidence,
                ),
                _ => bail!("Expected values must be booleans or choices"),
            };
            if !accepted {
                abstained += 1
            } else if matches {
                correct += 1
            } else {
                incorrect += 1
            }
            judgments.push(json!({"id":id,"expected":expected,"actual":answer,"correct":matches,"accepted":accepted}));
        }
        let box_result = if let Some([x0, y0, x1, y1]) = case.click_rect {
            let (w, h) = api::dimensions(&copy)?;
            ensure!(
                x0 < x1 && y0 < y1 && x1 <= w && y1 <= h,
                "Invalid labeled click region"
            );
            let point = api::center(&response, "target", 0., w, h)?;
            Some(
                json!({"center":point,"center_inside_label":point.0>=x0 && point.0<x1 && point.1>=y0 && point.1<y1,"accepted":api::center(&response,"target",corpus.min_confidence,w,h).is_ok(),"answer":response["answers"]["target"]}),
            )
        } else {
            None
        };
        results.push(json!({"id":case.id,"original":image,"sha256":hash(&copy)?,"judgments":judgments,"box":box_result}));
        println!("{}: recorded", case.id);
    }
    save(
        &out.join("report.json"),
        &json!({"kind":"labeled-screenshot-calibration","description":corpus.description,"correct_accepted":correct,"incorrect_accepted":incorrect,"abstained":abstained,"results":results,"metrics":client.summary(),"qualification":"not applicable; historical screenshot corpus, not live UI tests"}),
    )?;
    Ok(())
}
