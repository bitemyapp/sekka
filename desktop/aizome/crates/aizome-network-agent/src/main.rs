use aizome_network_agent::{keyring, model::Prompt, process::MAX_BYTES, prompt::Dialog};
use std::io::Read;

fn input<T: serde::de::DeserializeOwned>() -> Result<T, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin().take((MAX_BYTES + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES {
        return Err("Credential request is too large".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Do not put credentials into core dumps.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0);
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match std::env::args().nth(1).as_deref() {
        Some("--keyring") => {
            let result = keyring::worker(input()?);
            match result {
                Ok(values) => serde_json::to_writer(std::io::stdout(), &values)?,
                Err(_) => std::process::exit(1),
            }
        }
        Some("--prompt") => {
            let prompt: Prompt = input()?;
            if prompt.fields.is_empty() || prompt.fields.len() > 16 {
                return Err("Invalid authentication prompt".into());
            }
            let dialog = aizome_kit::try_run(Dialog { prompt, answer: None })?;
            serde_json::to_writer(std::io::stdout(), &dialog.answer)?;
        }
        None => aizome_network_agent::serve()?,
        Some("--help") => println!("aizome-network-agent: Aizome's session network credential service"),
        Some(_) => return Err("Unknown argument".into()),
    }
    Ok(())
}
