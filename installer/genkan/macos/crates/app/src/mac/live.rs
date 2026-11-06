// SPDX-License-Identifier: GPL-3.0-or-later
//! The real Mac behind [`Environment`]: the bundled engine, Apple's URL
//! loading system, `SMAppService` and the helper's XPC channel. Every call
//! here may block, and runs on a worker thread (see `runtime`), except the
//! few marked for the main thread.
use objc2::AnyThread;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{
    NSAppleScript, NSBundle, NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString,
    NSURL, NSURLRequestCachePolicy, NSURLResponse, NSURLSession, NSFullUserName, NSUserName,
};
use objc2_service_management::{SMAppService, SMAppServiceStatus};
use sekka_mac_contract::{
    ArtifactRole, Begin, HELPER_SERVICE, HelperRefusal,
    HelperRefusalCode, HelperRequest, HelperRequestSchema, HelperResponse, HelperResponseSchema,
    HttpsUrl, Inspection, Release, Request, Response, Summary, Timestamp, Uuid, canonical, limits,
};
use sekka_mac_stage::{Expect, RangeResponse, StageEvent, Stager, StagingDir, Transport, TransportError};
use sekka_mac_trust::{AcceptedStore, TrustConfig, Verified};

use crate::env::*;

/// The team that signs the helper, fixed at build time; the app talks only
/// to a helper with this signature.
const TEAM: Option<&str> = option_env!("SEKKA_TEAM_ID");
#[cfg(feature = "production")]
const _: () = assert!(
    option_env!("SEKKA_TEAM_ID").is_some(),
    "production apps name the team whose helper they talk to (SEKKA_TEAM_ID)"
);
const HELPER_PLIST: &str = "computer.sekka.installer.helper.plist";
const HELPER_IDENTIFIER: &str = "computer.sekka.installer.helper";
/// Where catalogs are published, by channel.
const CATALOG_BASE: &str = "https://downloads.sekka.computer/mac/v1";
const CHANNEL: &str = if cfg!(feature = "production") { "stable" } else { "development" };

pub struct Live {
    support: PathBuf,
    caches: PathBuf,
    engine: PathBuf,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

impl Default for Live {
    fn default() -> Self {
        Self::new()
    }
}

impl Live {
    pub fn new() -> Self {
        let resources = NSBundle::mainBundle()
            .resourcePath()
            .map(|p| PathBuf::from(p.to_string()))
            .unwrap_or_default();
        Self {
            support: home().join("Library/Application Support/Sekka Installer"),
            caches: home().join("Library/Caches/computer.sekka.installer"),
            engine: resources.join("Engine/engine.tar.gz"),
        }
    }

    fn staging(&self) -> Result<StagingDir, String> {
        StagingDir::open(&self.support.join("Staging")).map_err(|e| e.to_string())
    }

    fn helper(&self, request: Request, password: Option<&[u8]>) -> Result<Response, String> {
        let client = sekka_mac_xpc::Client::connect(
            HELPER_SERVICE,
            &sekka_mac_xpc::requirement(HELPER_IDENTIFIER, TEAM),
        )
        .map_err(|e| e.to_string())?;
        let bytes = canonical::to_vec(&HelperRequest {
            schema: HelperRequestSchema,
            id: 1,
            request,
        })
        .map_err(|e| e.to_string())?;
        let reply = client.send(&bytes, password).map_err(|e| e.to_string())?;
        let reply: HelperResponse =
            canonical::from_slice(&reply, limits::HELPER_MESSAGE, HelperResponseSchema::VALUE)
                .map_err(|e| format!("The helper's answer couldn't be read: {e}"))?;
        Ok(reply.response)
    }

    fn service() -> Retained<SMAppService> {
        unsafe { SMAppService::daemonServiceWithPlistName(&NSString::from_str(HELPER_PLIST)) }
    }
}

fn refusal(message: String) -> HelperRefusal {
    HelperRefusal {
        code: HelperRefusalCode::Unknown,
        message: message.chars().take(2000).collect::<String>().parse().unwrap_or_else(|_| "The helper didn't answer.".parse().unwrap()),
    }
}

/// One GET through Apple's URL loading system: its trust store, proxies and
/// idle timeout.
fn get(url: &str, range: Option<(u64, u64)>) -> Result<(u16, Option<String>, Vec<u8>), TransportError> {
    let nsurl = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| TransportError::Fatal(format!("not an address: {url}")))?;
    let request = NSMutableURLRequest::requestWithURL(&nsurl);
    if let Some((start, end)) = range {
        request.setValue_forHTTPHeaderField(
            Some(&NSString::from_str(&format!("bytes={start}-{end}"))),
            &NSString::from_str("Range"),
        );
    }
    request.setValue_forHTTPHeaderField(Some(&NSString::from_str("identity")), &NSString::from_str("Accept-Encoding"));
    request.setTimeoutInterval(60.0);
    request.setCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
    let (tx, rx) = channel();
    let block = RcBlock::new(move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
        // SAFETY: NSURLSession passes valid objects or null.
        let result = unsafe {
            if let Some(error) = error.as_ref() {
                Err(TransportError::Retryable(error.localizedDescription().to_string()))
            } else {
                let http = response.as_ref().and_then(|r| r.downcast_ref::<NSHTTPURLResponse>());
                match http {
                    None => Err(TransportError::Fatal("not an HTTP response".into())),
                    Some(http) => {
                        let status = http.statusCode() as u16;
                        let range = http
                            .valueForHTTPHeaderField(&NSString::from_str("Content-Range"))
                            .map(|v| v.to_string());
                        let bytes = data.as_ref().map(|d| d.to_vec()).unwrap_or_default();
                        Ok((status, range, bytes))
                    }
                }
            }
        };
        let _ = tx.send(result);
    });
    let task = unsafe { NSURLSession::sharedSession().dataTaskWithRequest_completionHandler(&request, &block) };
    task.resume();
    rx.recv().unwrap_or(Err(TransportError::Retryable("the request was dropped".into())))
}

struct Session;
impl Transport for Session {
    fn get_range(&self, url: &HttpsUrl, start: u64, end: u64) -> Result<RangeResponse, TransportError> {
        let (status, content_range, body) = get(url.as_str(), Some((start, end)))?;
        Ok(RangeResponse {
            status,
            content_range,
            body,
        })
    }
}

impl Environment for Live {
    fn user(&self) -> (String, String) {
        (NSUserName().to_string(), NSFullUserName().to_string())
    }

    fn inspect(&self) -> Result<Inspection, String> {
        let engine = sekka_mac_engine::unpack(&self.engine, &self.caches.join("engine"), None).map_err(|e| e.to_string())?;
        sekka_mac_engine::inspect(&engine).map_err(|e| e.to_string())
    }

    fn catalog(&self) -> Result<Verified, String> {
        let bytes = match catalog_override() {
            Some(path) => fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?,
            None => {
                let url = format!("{CATALOG_BASE}/catalog-{CHANNEL}.signed.json");
                match get(&url, None) {
                    Ok((200, _, bytes)) => bytes,
                    Ok((404, _, _)) => return Err("There's no Mac release on this channel yet.".into()),
                    Ok((status, _, _)) => return Err(format!("Sekka's release list couldn't be downloaded (the server answered {status}). Check again later.")),
                    Err(e) => return Err(format!("Sekka's release list couldn't be downloaded. Check your internet connection, then check again. ({e})")),
                }
            }
        };
        let now = Timestamp::now();
        let verified = sekka_mac_trust::verify(&bytes, &TrustConfig::for_this_build(), now).map_err(|e| {
            let mut s = e.to_string();
            if let Some(first) = s.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            format!("{s}.")
        })?;
        let mut store = AcceptedStore::open(&self.support.join("accepted-catalogs.json")).map_err(|e| e.to_string())?;
        store.accept(&verified, now).map_err(|e| e.to_string())?;
        Ok(verified)
    }

    fn helper_status(&self) -> Result<Vec<Summary>, HelperProblem> {
        match self.helper(Request::Status, None) {
            Ok(Response::Status { installations }) => Ok(installations),
            Ok(Response::Refused(r)) => Err(HelperProblem::Other(r.message.to_string())),
            Ok(_) => Err(HelperProblem::Other("The helper answered something unexpected.".into())),
            Err(_) => Err(HelperProblem::Unavailable),
        }
    }

    fn still_to_download(&self, release: &Release) -> u64 {
        let Ok(dir) = self.staging() else { return release.download_bytes() };
        let a = &release.artifacts;
        let files = [
            ("payload.zip", Expect::from(&a.payload)),
            ("installer_data.json", Expect::from(&a.installer_data)),
            ("manifest.json", Expect::from(&a.manifest)),
            ("recovery.zip", Expect::from(&a.recovery_app)),
        ];
        let refs: Vec<(&str, &Expect)> = files.iter().map(|(n, e)| (*n, e)).collect();
        dir.bytes_needed(&refs)
            + release.apple_inputs.iter().map(|i| i.approximate_bytes).sum::<u64>()
    }

    fn download(
        &self,
        release: &Release,
        cancel: &AtomicBool,
        on: &mut dyn FnMut(DownloadProgress),
    ) -> Result<Vec<Staged>, String> {
        let dir = self.staging()?;
        let stager = Stager::new(&dir, Session, cancel);
        let a = &release.artifacts;
        let files = [
            (ArtifactRole::InstallerData, "installer_data.json", &a.installer_data),
            (ArtifactRole::Manifest, "manifest.json", &a.manifest),
            (ArtifactRole::RecoveryApp, "recovery.zip", &a.recovery_app),
            (ArtifactRole::Payload, "payload.zip", &a.payload),
        ];
        let total: u64 = files.iter().map(|(_, _, f)| f.size).sum();
        let mut before = 0u64;
        let mut staged = Vec::new();
        for (role, name, artifact) in files {
            let file = stager
                .stage(name, &artifact.url, &Expect::from(artifact), &mut |event| match event {
                    StageEvent::Progress { done, .. } => on(DownloadProgress {
                        role,
                        done: before + done,
                        total,
                        retrying: None,
                        verifying: false,
                    }),
                    StageEvent::Retrying { reason, .. } => on(DownloadProgress {
                        role,
                        done: before,
                        total,
                        retrying: Some(reason),
                        verifying: false,
                    }),
                    StageEvent::Verifying => on(DownloadProgress {
                        role,
                        done: before + artifact.size,
                        total,
                        retrying: None,
                        verifying: true,
                    }),
                    StageEvent::Reused => {}
                })
                .map_err(|e| {
                    let mut s = e.to_string();
                    if let Some(first) = s.get_mut(0..1) {
                        first.make_ascii_uppercase();
                    }
                    format!("{s}.")
                })?;
            before += artifact.size;
            staged.push(Staged { role, file });
        }
        Ok(staged)
    }

    fn registration(&self) -> Registration {
        let status = unsafe { Self::service().status() };
        if status == SMAppServiceStatus::Enabled {
            Registration::Ready
        } else if status == SMAppServiceStatus::RequiresApproval {
            Registration::NeedsApproval
        } else {
            Registration::Absent
        }
    }

    fn register_helper(&self) -> Result<Registration, String> {
        let service = Self::service();
        if let Err(e) = unsafe { service.registerAndReturnError() } {
            if self.registration() == Registration::NeedsApproval {
                return Ok(Registration::NeedsApproval);
            }
            return Err(e.localizedDescription().to_string());
        }
        Ok(self.registration())
    }

    fn open_login_items(&self) {
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }

    fn begin(&self, begin: Begin, password: Secret) -> Result<Uuid, HelperRefusal> {
        match self.helper(Request::Begin(Box::new(begin)), Some(&password)) {
            Ok(Response::Begun { installation }) => Ok(installation),
            Ok(Response::Refused(r)) => Err(r),
            Ok(_) => Err(refusal("The helper answered something unexpected.".into())),
            Err(e) => Err(refusal(e)),
        }
    }

    fn events(&self, installation: Uuid, after: u64) -> Result<Events, String> {
        match self.helper(Request::Events { installation, after }, None)? {
            Response::Events {
                records,
                state,
                running,
                progress,
            } => Ok(Events {
                records,
                state,
                running,
                progress,
            }),
            Response::Refused(r) => Err(r.message.to_string()),
            _ => Err("The helper answered something unexpected.".into()),
        }
    }

    fn cancel(&self, installation: Uuid) -> Result<(), HelperRefusal> {
        match self.helper(Request::Cancel { installation }, None) {
            Ok(Response::Cancelled) => Ok(()),
            Ok(Response::Refused(r)) => Err(r),
            Ok(_) => Err(refusal("The helper answered something unexpected.".into())),
            Err(e) => Err(refusal(e)),
        }
    }

    fn retry_startup_disk(&self, installation: Uuid, password: Secret) -> Result<Events, HelperRefusal> {
        match self.helper(Request::RetryStartupDisk { installation }, Some(&password)) {
            Ok(Response::Events {
                records,
                state,
                running,
                progress,
            }) => Ok(Events {
                records,
                state,
                running,
                progress,
            }),
            Ok(Response::Refused(r)) => Err(r),
            Ok(_) => Err(refusal("The helper answered something unexpected.".into())),
            Err(e) => Err(refusal(e)),
        }
    }

    fn diagnostics(&self, installation: Uuid) -> Vec<String> {
        match self.helper(Request::Diagnostics { installation }, None) {
            Ok(Response::Diagnostics { lines }) => lines.into_iter().map(|l| l.to_string()).collect(),
            Ok(other) => vec![format!("The helper answered: {other:?}")],
            Err(e) => vec![format!("The helper couldn't be reached: {e}")],
        }
    }

    fn remove_staging(&self) {
        if let Ok(dir) = self.staging() {
            for name in ["payload.zip", "installer_data.json", "manifest.json", "recovery.zip"] {
                let _ = fs::remove_file(dir.path().join(name));
                let _ = fs::remove_file(dir.path().join(format!("{name}.partial")));
            }
        }
    }

    fn copy_to_pasteboard(&self, text: &str) {
        let pasteboard = objc2_app_kit::NSPasteboard::generalPasteboard();
        pasteboard.clearContents();
        pasteboard.setString_forType(&NSString::from_str(text), unsafe { objc2_app_kit::NSPasteboardTypeString });
    }

    fn shut_down(&self) -> Result<(), String> {
        // The standard Shut Down confirmation, as the Apple menu shows it.
        let source = NSString::from_str("tell application \"loginwindow\" to «event aevtrsdn»");
        let script = NSAppleScript::initWithSource(NSAppleScript::alloc(), &source)
            .ok_or("macOS couldn't prepare the shutdown request")?;
        let mut error = None;
        let result = unsafe { script.executeAndReturnError(Some(&mut error)) };
        let _ = result;
        match error {
            Some(_) => Err("macOS didn't accept the request".into()),
            None => Ok(()),
        }
    }
}

/// Development builds may read their catalog from a file
/// (`SEKKA_INSTALLER_CATALOG`), still verified with the development key.
/// Production builds read only Sekka's published catalog.
fn catalog_override() -> Option<PathBuf> {
    if cfg!(feature = "production") {
        return None;
    }
    std::env::var_os("SEKKA_INSTALLER_CATALOG").map(PathBuf::from)
}
