// SPDX-License-Identifier: GPL-3.0-or-later
//! Runs effects on worker threads and reports back as messages. The main
//! thread only ever drains the channel (a timer, see `mac::window`); nothing
//! here blocks it.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::env::{Environment, HelperProblem};
use crate::model::{Effect, Msg};

pub struct Runtime {
    env: Arc<dyn Environment>,
    tx: Sender<Msg>,
    stop_download: Arc<AtomicBool>,
}

impl Runtime {
    pub fn new(env: Arc<dyn Environment>, tx: Sender<Msg>) -> Self {
        Self {
            env,
            tx,
            stop_download: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn env(&self) -> &Arc<dyn Environment> {
        &self.env
    }

    fn spawn(&self, name: &str, work: impl FnOnce(&dyn Environment, &Sender<Msg>) + Send + 'static) {
        let env = self.env.clone();
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || work(env.as_ref(), &tx))
            .expect("a worker thread starts");
    }

    /// Start an effect. Effects that belong on the main thread (the
    /// pasteboard, System Settings, shutting down) are returned to the
    /// caller instead.
    pub fn run(&self, effect: Effect) -> Option<Effect> {
        match effect {
            Effect::Inspect => self.spawn("inspect", |env, tx| {
                let _ = tx.send(Msg::Inspected(env.inspect()));
            }),
            Effect::LoadCatalog => self.spawn("catalog", |env, tx| {
                let _ = tx.send(Msg::Catalog(env.catalog()));
            }),
            Effect::HelperStatus => self.spawn("helper status", |env, tx| {
                let status = match env.registration() {
                    crate::env::Registration::Ready => env.helper_status(),
                    _ => Err(HelperProblem::Unavailable),
                };
                let _ = tx.send(Msg::HelperStatus(status));
                let _ = tx.send(Msg::Registration(Ok(env.registration())));
            }),
            Effect::MeasureDownload(release) => self.spawn("measure", move |env, tx| {
                let _ = tx.send(Msg::ToDownload(env.still_to_download(&release)));
            }),
            Effect::Download(release) => {
                self.stop_download.store(false, Ordering::SeqCst);
                let stop = self.stop_download.clone();
                self.spawn("download", move |env, tx| {
                    let result = env.download(&release, &stop, &mut |p| {
                        let _ = tx.send(Msg::DownloadProgress(p));
                    });
                    let _ = tx.send(Msg::Downloaded(result));
                });
            }
            Effect::StopDownload => self.stop_download.store(true, Ordering::SeqCst),
            Effect::CheckRegistration => self.spawn("registration", |env, tx| {
                let _ = tx.send(Msg::Registration(Ok(env.registration())));
            }),
            Effect::RegisterHelper => self.spawn("register", |env, tx| {
                let _ = tx.send(Msg::Registration(env.register_helper()));
            }),
            Effect::Begin(begin, password) => self.spawn("begin", move |env, tx| {
                let _ = tx.send(Msg::Begun(env.begin(*begin, password)));
            }),
            Effect::Poll {
                installation,
                after,
                delay_ms,
            } => self.spawn("events", move |env, tx| {
                if delay_ms > 0 {
                    std::thread::sleep(Duration::from_millis(delay_ms));
                }
                let _ = tx.send(Msg::Events(env.events(installation, after)));
            }),
            Effect::Cancel(id) => self.spawn("cancel", move |env, tx| {
                let _ = tx.send(Msg::CancelAnswered(env.cancel(id)));
            }),
            Effect::RetryStartupDisk(id, password) => self.spawn("retry", move |env, tx| {
                let _ = tx.send(Msg::Retried(env.retry_startup_disk(id, password)));
            }),
            Effect::Diagnostics(id) => self.spawn("diagnostics", move |env, tx| {
                let _ = tx.send(Msg::Diagnostics(env.diagnostics(id)));
            }),
            Effect::RemoveStaging => self.spawn("cleanup", |env, _| env.remove_staging()),
            main @ (Effect::OpenLoginItems | Effect::CopyToPasteboard(_) | Effect::ShutDown) => return Some(main),
        }
        None
    }
}
