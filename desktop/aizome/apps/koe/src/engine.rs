use crate::{
    audio,
    config::{Backend, Engine},
};
use anyhow::{Context, Result, bail, ensure};
#[cfg(feature = "local")]
use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub struct Recognizer {
    config: Engine,
    #[cfg(feature = "local")]
    context: Arc<Mutex<Option<Resident>>>,
    loaded: Arc<AtomicBool>,
    /// The resident model runs on a graphics card.
    gpu: Arc<AtomicBool>,
}

/// Whether whisper.cpp has a graphics card to run on: a device of ggml's it
/// would choose (a GPU, or an integrated one). A Vulkan build without a
/// working driver has none, and then the GPU settings only slow the
/// processor down: flash attention there is slower than OpenBLAS's kernels.
#[cfg(feature = "local")]
pub fn gpu_available() -> bool {
    use whisper_rs::whisper_rs_sys as sys;
    // SAFETY: ggml's registry lists only the backends that started (it
    // catches a Vulkan instance that can't be made); these calls read it.
    unsafe {
        (0..sys::ggml_backend_dev_count()).any(|i| {
            matches!(
                sys::ggml_backend_dev_type(sys::ggml_backend_dev_get(i)),
                sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU
                    | sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU
            )
        })
    }
}
#[cfg(not(feature = "local"))]
pub fn gpu_available() -> bool {
    false
}
#[cfg(feature = "local")]
struct Resident {
    model: whisper_rs::WhisperContext,
    state: whisper_rs::WhisperState,
}

#[cfg(feature = "local")]
unsafe extern "C" fn should_abort(data: *mut std::ffi::c_void) -> bool {
    // SAFETY: local() passes an Arc-owned AtomicBool, kept alive until full()
    // returns. The callback only reads it and never accesses Whisper state.
    unsafe { &*data.cast::<AtomicBool>() }.load(Ordering::Relaxed)
}
impl Recognizer {
    pub fn new(config: Engine) -> Self {
        // Native debug logs can contain decoded tokens. Keep them out of the
        // journal as well as stdout, even in development builds.
        #[cfg(feature = "local")]
        whisper_rs::install_logging_hooks();
        Self {
            config,
            #[cfg(feature = "local")]
            context: Arc::new(Mutex::new(None)),
            loaded: Arc::new(AtomicBool::new(false)),
            gpu: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn loaded(&self) -> bool {
        self.loaded.load(Ordering::Relaxed)
    }
    /// Whether the resident model runs on a graphics card.
    pub fn on_gpu(&self) -> bool {
        self.loaded() && self.gpu.load(Ordering::Relaxed)
    }
    /// Per-utterance vocabulary changes reuse the resident weights and buffers.
    pub fn with_prompt(&self, prompt: String) -> Self {
        let mut recognizer = self.clone();
        recognizer.config.prompt = prompt;
        recognizer
    }
    /// Prepare model buffers and GPU pipelines before the first dictation.
    /// Silence is discarded and no_context prevents it entering later prompts.
    pub async fn warmup(&self) -> Result<()> {
        if self.config.backend == Backend::Local && self.config.model.is_file() {
            let mut warmup = self.clone();
            // Silence has no language to detect. Use a fixed language only for
            // this discarded warmup; real dictation retains the user's setting.
            warmup.config.language = "en".into();
            warmup.config.prompt.clear();
            warmup.transcribe(vec![0.0; audio::RATE], Arc::new(AtomicBool::new(false))).await?;
        }
        Ok(())
    }
    pub fn unload(&self) -> Result<()> {
        #[cfg(feature = "local")]
        {
            *self.context.try_lock().map_err(|_| anyhow::anyhow!("model is busy"))? = None;
        }
        self.loaded.store(false, Ordering::Relaxed);
        Ok(())
    }
    pub async fn transcribe(&self, samples: Vec<f32>, cancel: Arc<AtomicBool>) -> Result<String> {
        ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
        match self.config.backend {
            Backend::Local => {
                let this = self.clone();
                tokio::task::spawn_blocking(move || this.local(&samples, cancel))
                    .await
                    .context("recognizer worker failed")?
            }
            Backend::Http => self.http(&samples, cancel).await,
        }
    }
    #[cfg(not(feature = "local"))]
    fn local(&self, _: &[f32], _: Arc<AtomicBool>) -> Result<String> {
        bail!("this build has no local backend; build with the default local feature");
    }
    #[cfg(feature = "local")]
    fn local(&self, samples: &[f32], cancel: Arc<AtomicBool>) -> Result<String> {
        use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};
        ensure!(
            self.config.language == "auto" || whisper_rs::get_lang_id(&self.config.language).is_some(),
            "unknown Whisper language code"
        );
        let mut context = self.context.lock().map_err(|_| anyhow::anyhow!("recognizer mutex poisoned"))?;
        ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
        if context.is_none() {
            ensure!(
                self.config.model.is_file(),
                "model not found: {}; download one in Koe",
                self.config.model.display()
            );
            // Flash attention only where there's a card to run it on.
            let gpu = self.config.gpu && gpu_available();
            let mut params = WhisperContextParameters::default();
            params.use_gpu(gpu);
            params.flash_attn(gpu);
            let loaded =
                WhisperContext::new_with_params(self.config.model.to_str().context("model path is not UTF-8")?, params)
                    .context("cannot load Whisper model")?;
            *context = Some(Resident { state: loaded.create_state()?, model: loaded });
            self.gpu.store(gpu, Ordering::Relaxed);
            self.loaded.store(true, Ordering::Relaxed);
        }
        ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
        // Keep the inference state resident as well as the weights. Recreating
        // it on every utterance repeats GPU allocation and pipeline setup.
        let resident = context.as_mut().unwrap();
        let hint_limit = resident.model.n_text_ctx() as usize / 2;
        let tokens = resident.model.tokenize(&self.config.prompt, self.config.prompt.len() + 1)?;
        ensure!(
            tokens.len() <= hint_limit,
            "vocabulary and engine.prompt exceed Whisper's {hint_limit}-token hint limit; disable a pack or shorten its words"
        );
        let state = &mut resident.state;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.config.threads as i32);
        params.set_language(if self.config.language == "auto" { None } else { Some(&self.config.language) });
        params.set_translate(self.config.translate);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_initial_prompt(&self.config.prompt);
        // whisper-rs 0.16's closure adapter boxes a trait object but casts its
        // pointer to the concrete closure type. Avoid that mismatched layout
        // (and leaked allocation) by passing a stable atomic directly.
        // SAFETY: cancel owns the pointee throughout this synchronous full()
        // call, including all callbacks. Whisper invokes this callback only
        // during the call; the function cannot panic or mutate model state.
        unsafe {
            params.set_abort_callback(Some(should_abort));
            params.set_abort_callback_user_data(Arc::as_ptr(&cancel).cast_mut().cast());
        }
        state.full(params, samples).context("Whisper inference failed")?;
        ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
        Ok(state.as_iter().map(|s| s.to_string()).collect::<String>())
    }
    async fn http(&self, samples: &[f32], cancel: Arc<AtomicBool>) -> Result<String> {
        let c = &self.config.http;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(c.timeout_seconds))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let file = reqwest::multipart::Part::bytes(audio::wav_bytes(samples)?)
            .file_name("dictation.wav")
            .mime_str("audio/wav")?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", file)
            .text("model", c.model.clone())
            .text("response_format", "json");
        if self.config.language != "auto" {
            form = form.text("language", self.config.language.clone());
        }
        if !self.config.prompt.is_empty() {
            form = form.text("prompt", self.config.prompt.clone());
        }
        if self.config.translate {
            form = form.text("translate", "true");
        }
        let mut request = client.post(&c.endpoint).multipart(form);
        if let Some(var) = &c.api_key_env {
            let key = std::env::var(var).with_context(|| format!("credential environment variable {var} is unset"))?;
            ensure!(!key.is_empty(), "credential environment variable is empty");
            request = request.bearer_auth(key);
        }
        // Do not include server response bodies or URLs in errors: they may contain speech or credentials.
        let mut response = request.send().await.map_err(|_| anyhow::anyhow!("transcription HTTP request failed"))?;
        ensure!(response.status().is_success(), "transcription server returned HTTP {}", response.status().as_u16());
        let mut bytes = Vec::new();
        while let Some(chunk) =
            response.chunk().await.map_err(|_| anyhow::anyhow!("cannot read transcription response"))?
        {
            ensure!(!cancel.load(Ordering::Relaxed), "cancelled");
            ensure!(bytes.len() + chunk.len() <= 1024 * 1024, "transcription response exceeds 1 MiB");
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).context("transcription response must be JSON")?;
        match value.get("text").and_then(|t| t.as_str()) {
            Some(text) => Ok(text.to_owned()),
            None => bail!("transcription response must have a string text field"),
        }
    }
}

#[cfg(all(test, feature = "local"))]
mod tests {
    use super::*;
    #[test]
    fn native_cancellation_callback_reads_the_owned_atomic() {
        let cancel = Arc::new(AtomicBool::new(false));
        let data = Arc::as_ptr(&cancel).cast_mut().cast();
        // SAFETY: the atomic remains alive throughout both calls.
        unsafe {
            assert!(!should_abort(data));
        }
        cancel.store(true, Ordering::Relaxed);
        unsafe {
            assert!(should_abort(data));
        }
    }
}
