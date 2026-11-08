//! UI-thread WebView2 child view. Only the genuine provider's rendered results are read.
use crate::{
    engine::Shared,
    model::{SpeedDetails, SpeedRun, now_ms},
    storage::Store,
};
use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, bounded};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
mod capture;
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    Win32::{
        Foundation::*,
        System::{Com::*, LibraryLoader::*},
        UI::Input::KeyboardAndMouse::{SetFocus, VK_F6},
    },
    core::*,
};

#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub download: f64,
    pub upload: f64,
    pub ping: f64,
    pub server: String,
    pub details: SpeedDetails,
}
const LOAD_TIMEOUT: Duration = Duration::from_secs(45);

/// Tracks only the current allowed navigation. Redirects share an ID and must
/// not restart the timeout; stale/canceled completion events cannot mark it ready.
struct Loading {
    since: Instant,
    navigation: Option<u64>,
    ready: bool,
}
impl Default for Loading {
    fn default() -> Self {
        Self {
            since: Instant::now(),
            navigation: None,
            ready: false,
        }
    }
}
impl Loading {
    fn begin(&mut self, navigation: u64, now: Instant) {
        if self.navigation != Some(navigation) {
            self.since = now;
            self.navigation = Some(navigation);
        }
        self.ready = false;
    }
    fn complete(&mut self, navigation: u64, success: bool) -> bool {
        if self.navigation != Some(navigation) {
            return false;
        }
        self.ready = success;
        true
    }
    fn timed_out(&self, now: Instant) -> bool {
        !self.ready && now.duration_since(self.since) >= LOAD_TIMEOUT
    }
}

#[derive(Default)]
struct ViewState {
    controller: Option<ICoreWebView2Controller>,
    webview: Option<ICoreWebView2>,
    error: Option<String>,
    closed: bool,
    loading: Loading,
}
impl ViewState {
    fn fail(&mut self, error: String) {
        if !self.closed && self.error.is_none() {
            self.loading.ready = false;
            self.error = Some(error);
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    WiFiman,
    Speedtest,
    Fast,
    Devina,
}
pub const PROVIDERS: [Provider; 4] = [
    Provider::WiFiman,
    Provider::Speedtest,
    Provider::Fast,
    Provider::Devina,
];
impl Provider {
    pub fn from_key(key: &str) -> Option<Self> {
        match key.to_ascii_lowercase().as_str() {
            "wifiman" => Some(Self::WiFiman),
            "speedtest" | "speedtest.net" => Some(Self::Speedtest),
            "fast" | "fast.com" => Some(Self::Fast),
            "devina" | "bufferbloat" => Some(Self::Devina),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::WiFiman => "WiFiman",
            Self::Speedtest => "Speedtest.net",
            Self::Fast => "FAST.com",
            Self::Devina => "Devina bufferbloat",
        }
    }
    pub fn url(self) -> &'static str {
        match self {
            Self::WiFiman => "https://wifiman.com/",
            Self::Speedtest => "https://www.speedtest.net/",
            Self::Fast => "https://fast.com/",
            Self::Devina => "https://devina.io/speed-test",
        }
    }
    fn allows(self, uri: &str) -> bool {
        let Ok(url) = url::Url::parse(uri) else {
            return false;
        };
        if url.scheme() != "https"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port_or_known_default() != Some(443)
        {
            return false;
        }
        match self {
            Self::WiFiman => url.host_str() == Some("wifiman.com") && url.path() == "/",
            Self::Speedtest => {
                matches!(url.host_str(), Some("www.speedtest.net" | "speedtest.net"))
            }
            Self::Fast => matches!(url.host_str(), Some("fast.com" | "www.fast.com")),
            Self::Devina => {
                url.host_str() == Some("devina.io")
                    && matches!(url.path(), "/speed-test" | "/speed-test/")
            }
        }
    }
}

/// A UI-thread native child browser, shared by the four provider panels.
pub struct Pane {
    state: Rc<RefCell<ViewState>>,
    bounds: Option<RECT>,
    visible: bool,
    apartment: bool,
}
impl Pane {
    pub fn open(directory: PathBuf, parent: isize, provider: Provider) -> Self {
        let state = Rc::new(RefCell::new(ViewState::default()));
        let apartment = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
        let pane = Self {
            state: state.clone(),
            bounds: None,
            visible: false,
            apartment,
        };
        let result = if apartment {
            initialize(directory, HWND(parent as *mut _), state.clone(), provider)
        } else {
            Err(anyhow::anyhow!(
                "WebView2 requires the owning STA UI thread"
            ))
        };
        if let Err(error) = result {
            state.borrow_mut().fail(format!("{error:#}"));
        }
        pane
    }
    pub fn status(&self) -> std::result::Result<bool, String> {
        self.service();
        let state = self.state.borrow();
        if let Some(error) = &state.error {
            Err(error.clone())
        } else {
            Ok(state.loading.ready && !state.closed)
        }
    }
    /// Perform COM cleanup from the ordinary UI loop, outside WebView callbacks.
    fn service(&self) {
        let failed = {
            let mut state = self.state.borrow_mut();
            if !state.closed && state.error.is_none() && state.loading.timed_out(Instant::now()) {
                let phase = if state.loading.navigation.is_some() {
                    "page loading"
                } else {
                    "browser initialization"
                };
                state.fail(format!("Provider {phase} timed out after 45 seconds. Check your connection, then close and reload this panel."));
            }
            state.error.is_some() && !state.closed
        };
        if failed {
            self.close();
        }
    }
    pub fn place(&mut self, rect: Option<[i32; 4]>) {
        self.service();
        let state = self.state.borrow();
        let Some(controller) = &state.controller else {
            return;
        };
        let visible = rect.is_some() && !state.closed;
        unsafe {
            if let Some([left, top, right, bottom]) = rect {
                let bounds = RECT {
                    left,
                    top,
                    right,
                    bottom,
                };
                if self.bounds != Some(bounds) {
                    let _ = controller.SetBounds(bounds);
                    self.bounds = Some(bounds);
                }
                let _ = controller.NotifyParentWindowPositionChanged();
            }
            if self.visible != visible {
                let _ = controller.SetZoomFactor(if visible { 0.25 } else { 1.0 });
                let _ = controller.SetIsVisible(visible);
                self.visible = visible;
                if !visible {
                    let bounds = RECT {
                        left: 0,
                        top: 0,
                        right: 1600,
                        bottom: 1000,
                    };
                    let _ = controller.SetBounds(bounds);
                    self.bounds = Some(bounds);
                }
            }
        }
    }
    fn close(&self) {
        let mut state = self.state.borrow_mut();
        state.closed = true;
        state.webview = None;
        let controller = state.controller.take();
        drop(state);
        if let Some(controller) = controller {
            unsafe {
                let _ = controller.Close();
            }
        }
    }
}
impl Drop for Pane {
    fn drop(&mut self) {
        self.close();
        if self.apartment {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

pub struct Browser {
    pub result: Receiver<BrowserResult>,
    tx: Sender<BrowserResult>,
    pane: Pane,
    run: Option<SpeedRun>,
    store: Store,
    network: Arc<Mutex<Shared>>,
    snapshots: Receiver<Snapshot>,
    snapshot_tx: Sender<Snapshot>,
    busy: Rc<std::cell::Cell<bool>>,
    gate: capture::ResultGate,
    provider: Provider,
    phase: String,
    last_parse_error: String,
    debug: Option<std::fs::File>,
    started: Instant,
    last_poll: Instant,
    saving: bool,
}
pub struct BrowserResult {
    pub run: SpeedRun,
    pub saved: bool,
    pub error: Option<String>,
}

pub fn start(
    directory: PathBuf,
    store: Store,
    run: SpeedRun,
    network: Arc<Mutex<Shared>>,
    parent: isize,
    provider: Provider,
) -> Browser {
    let (tx, result) = bounded(1);
    let (snapshot_tx, snapshots) = bounded(4);
    let debug = std::env::args()
        .any(|arg| arg == "--speed-debug")
        .then(|| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join("provider-debug.jsonl"))
                .ok()
        })
        .flatten();
    let pane = Pane::open(directory, parent, provider);
    let mut view = Browser {
        result,
        tx,
        pane,
        run: Some(run),
        store,
        network,
        snapshots,
        snapshot_tx,
        busy: Rc::new(std::cell::Cell::new(false)),
        gate: capture::ResultGate::default(),
        provider,
        phase: "Loading provider".into(),
        last_parse_error: String::new(),
        debug,
        started: Instant::now(),
        last_poll: Instant::now() - Duration::from_secs(1),
        saving: false,
    };
    if let Err(error) = view.pane.status() {
        view.finish(Err(error), false);
    }
    view
}

impl Browser {
    pub fn provider(&self) -> Provider {
        self.provider
    }
    pub fn phase(&self) -> &str {
        &self.phase
    }
    pub fn is_saving(&self) -> bool {
        self.saving
    }
    pub fn take_result(&mut self) -> Option<BrowserResult> {
        let result = self.result.try_recv().ok();
        if result.is_some() {
            self.saving = false;
        }
        result
    }
    /// Called on eframe's owning UI thread; winit pumps WebView2's async callbacks.
    pub fn tick(&mut self) {
        if self.run.is_none() {
            return;
        }
        match self.pane.status() {
            Err(error) => {
                self.finish(Err(error), false);
                return;
            }
            Ok(false) => return,
            Ok(true) => {}
        }
        if self.started.elapsed() > Duration::from_secs(300) {
            self.finish(
                Err(format!(
                    "No complete new result within five minutes. {}",
                    self.last_parse_error
                )),
                false,
            );
            return;
        }
        let webview = self.pane.state.borrow().webview.clone();
        let Some(webview) = webview else {
            return;
        };
        while let Ok(snapshot) = self.snapshots.try_recv() {
            // Initial about:blank is expected during asynchronous navigation.
            if snapshot.origin == "null" || snapshot.origin.is_empty() {
                continue;
            }
            let mut source = PWSTR::null();
            let source_result = unsafe { webview.Source(&mut source) };
            let source = CoTaskMemPWSTR::from(source).to_string();
            let origin = url::Url::parse(&source)
                .ok()
                .map(|url| url.origin().ascii_serialization());
            if source_result.is_err()
                || origin.as_deref() != Some(&snapshot.origin)
                || !self.provider.allows(&source)
            {
                self.finish(Err("Unexpected provider navigation".into()), false);
                return;
            }
            self.phase = snapshot.phase.clone();
            if let Some(link) = snapshot.fields.get("result_url")
                && self.provider.allows(link)
                && let Some(run) = self.run.as_mut()
            {
                run.result_url = link.clone();
            }
            if let Some(file) = &mut self.debug {
                let _ = writeln!(
                    file,
                    "{}",
                    serde_json::json!({"provider":format!("{:?}",self.provider),"at":now_ms(),"snapshot":snapshot})
                );
            }
            match self.gate.observe(self.provider, &snapshot) {
                Ok(Some(capture)) => {
                    self.finish(Ok(capture), false);
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    self.last_parse_error = error.to_string();
                    self.phase = "Waiting for all completed measurements".into();
                }
            }
        }
        if self.last_poll.elapsed() >= Duration::from_millis(500) && !self.busy.get() {
            self.busy.set(true);
            let busy = self.busy.clone();
            let tx = self.snapshot_tx.clone();
            let callback =
                ExecuteScriptCompletedHandler::create(Box::new(move |status, result| {
                    busy.set(false);
                    if status.is_ok()
                        && let Ok(snapshot) = serde_json::from_str::<Snapshot>(&result)
                    {
                        let _ = tx.try_send(snapshot);
                    }
                    Ok(())
                }));
            let script = include_str!("speed/adapter.js").replace(
                "__PROVIDER__",
                &format!("{:?}", format!("{:?}", self.provider)),
            );
            let script: Vec<u16> = script.encode_utf16().chain(Some(0)).collect();
            if let Err(error) = unsafe { webview.ExecuteScript(PCWSTR(script.as_ptr()), &callback) }
            {
                self.busy.set(false);
                self.finish(Err(error.to_string()), false);
            }
            self.last_poll = Instant::now();
        }
    }
    pub fn place(&mut self, rect: Option<[i32; 4]>) {
        self.pane.place(rect.filter(|_| self.run.is_some()));
    }
    pub fn status(&self) -> std::result::Result<bool, String> {
        self.pane.status()
    }
    pub fn cancel(&mut self) {
        self.finish(Err("Test canceled".into()), true);
    }
    fn close_view(&mut self) {
        self.pane.close();
    }
    fn finish(&mut self, capture: std::result::Result<Capture, String>, canceled: bool) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        self.close_view();
        let mut error = None;
        match capture {
            Ok(c) => {
                run.status = "completed".into();
                run.download_mbps = Some(c.download);
                run.upload_mbps = Some(c.upload);
                run.ping_ms = Some(c.ping);
                run.server = c.server;
                run.details = c.details;
            }
            Err(reason) => {
                run.status = if canceled {
                    "canceled"
                } else {
                    "capture_failed"
                }
                .into();
                run.details.error = reason.clone();
                error = Some(reason);
            }
        }
        run.finished_ms = Some(now_ms());
        run.end_context = self
            .network
            .lock()
            .unwrap()
            .targets
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}: {:?}", crate::model::TARGET_NAMES[i], t.network))
            .collect::<Vec<_>>()
            .join("; ");
        let store = self.store.clone();
        let tx = self.tx.clone();
        self.saving = true;
        // The writer commits even when the tab is hidden or the application is minimized.
        std::thread::spawn(move || {
            let saved = match store.speed(run.clone()) {
                Ok(()) => true,
                Err(reason) => {
                    error = Some(format!("Result NOT saved: {reason}"));
                    false
                }
            };
            let _ = tx.send(BrowserResult { run, saved, error });
        });
    }
}
impl Drop for Browser {
    fn drop(&mut self) {
        if self.run.is_some() {
            self.cancel();
        }
        if self.saving {
            let _ = self.result.recv_timeout(Duration::from_secs(10));
        }
        self.close_view();
    }
}
#[derive(Debug, Serialize, Deserialize)]
struct Snapshot {
    origin: String,
    text: String,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    complete: bool,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    fields: BTreeMap<String, String>,
}

fn navigation_error(error: COREWEBVIEW2_WEB_ERROR_STATUS) -> String {
    let message = match error {
        COREWEBVIEW2_WEB_ERROR_STATUS_HOST_NAME_NOT_RESOLVED => {
            "the provider address could not be resolved (DNS)"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_DISCONNECTED => "the network connection is offline",
        COREWEBVIEW2_WEB_ERROR_STATUS_CANNOT_CONNECT
        | COREWEBVIEW2_WEB_ERROR_STATUS_SERVER_UNREACHABLE => "the provider could not be reached",
        COREWEBVIEW2_WEB_ERROR_STATUS_TIMEOUT => "the connection timed out",
        COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_ABORTED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CONNECTION_RESET => "the connection was interrupted",
        COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_COMMON_NAME_IS_INCORRECT
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_EXPIRED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_IS_INVALID
        | COREWEBVIEW2_WEB_ERROR_STATUS_CERTIFICATE_REVOKED
        | COREWEBVIEW2_WEB_ERROR_STATUS_CLIENT_CERTIFICATE_CONTAINS_ERRORS => {
            "the provider certificate could not be verified"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_VALID_PROXY_AUTHENTICATION_REQUIRED => {
            "the network proxy requires authentication"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_VALID_AUTHENTICATION_CREDENTIALS_REQUIRED => {
            "the provider requires authentication"
        }
        COREWEBVIEW2_WEB_ERROR_STATUS_OPERATION_CANCELED => "navigation was canceled",
        COREWEBVIEW2_WEB_ERROR_STATUS_REDIRECT_FAILED => "the provider redirect failed",
        COREWEBVIEW2_WEB_ERROR_STATUS_ERROR_HTTP_INVALID_SERVER_RESPONSE => {
            "the provider returned an invalid response"
        }
        _ => "navigation failed",
    };
    format!("{message} (WebView2 error {})", error.0)
}

fn process_error(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> &'static str {
    match kind {
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => "the browser process exited",
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
            "the page stopped responding"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED
        | COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED => {
            "the page renderer exited"
        }
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED => "the graphics process exited",
        _ => "a browser component stopped unexpectedly",
    }
}

pub fn parse_result(text: &str) -> Result<Capture> {
    anyhow::ensure!(
        text.contains("Test Result") && text.contains("This device") && text.contains("Internet"),
        "Unrecognized WiFiman result page"
    );
    let speed = regex::Regex::new(r"(?m)^\s*([0-9]+(?:\.[0-9]+)?)\s*(Mbps|Gbps)\s*$")?;
    let values: Vec<f64> = speed
        .captures_iter(text)
        .map(|m| m[1].parse::<f64>().unwrap_or(f64::NAN) * if &m[2] == "Gbps" { 1000. } else { 1. })
        .collect();
    anyhow::ensure!(
        values.len() == 2,
        "Expected exactly two completed throughput values"
    );
    let ping = regex::Regex::new(r"Ping:\s*([0-9]+(?:\.[0-9]+)?)\s*ms")?
        .captures(text)
        .context("Missing completed ping value")?[1]
        .parse::<f64>()?;
    anyhow::ensure!(
        values.iter().all(|v| v.is_finite() && *v >= 0.) && ping.is_finite() && ping >= 0.,
        "Invalid provider measurement"
    );
    // This observed page version renders the down-arrow/download first, up-arrow/upload second.
    // Unknown layouts deliberately fail rather than consulting private JS state.
    let server = text
        .split("Test Server\n")
        .nth(1)
        .and_then(|s| s.lines().next())
        .unwrap_or("")
        .trim()
        .to_string();
    Ok(Capture {
        download: values[0],
        upload: values[1],
        ping,
        server,
        details: SpeedDetails::default(),
    })
}

fn initialize(
    directory: PathBuf,
    parent: HWND,
    state: Rc<RefCell<ViewState>>,
    provider: Provider,
) -> Result<()> {
    let loader_dir = directory.join("browser-loader-1.0.3800.47");
    std::fs::create_dir_all(&loader_dir)?;
    let loader = loader_dir.join("WebView2Loader.dll");
    let bytes = include_bytes!("../assets/WebView2Loader.dll");
    if std::fs::read(&loader).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::write(&loader, bytes)?;
    }
    let loader_path: Vec<u16> = loader
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // Keep a single process-lifetime loader reference, including across close/reload.
    // Pending asynchronous environment creation must never outlive an unloaded loader.
    static LOADER_MODULE: OnceLock<usize> = OnceLock::new();
    let module = if let Some(address) = LOADER_MODULE.get() {
        HMODULE(*address as *mut _)
    } else {
        // Load only the embedded, signed Microsoft loader by absolute path.
        // This is not the browser runtime.
        let loaded = unsafe {
            LoadLibraryExW(
                PCWSTR(loader_path.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            )
        }?;
        if LOADER_MODULE.set(loaded.0 as usize).is_err() {
            unsafe {
                let _ = FreeLibrary(loaded);
            }
        }
        HMODULE(*LOADER_MODULE.get().expect("Loader reference initialized") as *mut _)
    };
    type VersionFn = unsafe extern "system" fn(PCWSTR, *mut PWSTR) -> HRESULT;
    type CreateFn = unsafe extern "system" fn(
        PCWSTR,
        PCWSTR,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> HRESULT;
    let version_address =
        unsafe { GetProcAddress(module, s!("GetAvailableCoreWebView2BrowserVersionString")) }
            .context("Missing runtime detector")?;
    let version: VersionFn = unsafe { std::mem::transmute(version_address) };
    let mut runtime = PWSTR::null();
    unsafe {
        version(PCWSTR::null(), &mut runtime)
            .ok()
            .context("WebView2 Runtime is unavailable; native monitoring continues")?;
        CoTaskMemFree(Some(runtime.0.cast()));
    }
    let create_address =
        unsafe { GetProcAddress(module, s!("CreateCoreWebView2EnvironmentWithOptions")) }
            .context("Missing browser factory")?;
    let create: CreateFn = unsafe { std::mem::transmute(create_address) };
    let data_path: Vec<u16> = directory
        .join("browser-automation-data")
        .to_string_lossy()
        .encode_utf16()
        .chain(Some(0))
        .collect();

    let weak = Rc::downgrade(&state);
    let handler = CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(
        move |status, environment| {
            let Some(state) = weak.upgrade() else {
                return Ok(());
            };
            if state.borrow().closed {
                return Ok(());
            }
            let initialized = (|| -> Result<()> {
                status?;
                let environment = environment.context("No WebView2 environment")?;
                let weak = Rc::downgrade(&state);
                let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                    move |status, controller| {
                        let Some(state) = weak.upgrade() else {
                            if let Some(c) = controller {
                                unsafe {
                                    let _ = c.Close();
                                }
                            }
                            return Ok(());
                        };
                        if state.borrow().closed {
                            if let Some(c) = controller {
                                unsafe {
                                    let _ = c.Close();
                                }
                            }
                            return Ok(());
                        }
                        let initialized = (|| -> Result<()> {
                            let controller = controller.context("No WebView2 controller")?;
                            let configured = (|| -> Result<ICoreWebView2> {
                                status?;
                                let webview = unsafe { controller.CoreWebView2()? };
                                configure(&controller, &webview, provider, parent, &state)?;
                                Ok(webview)
                            })();
                            let webview = match configured {
                                Ok(webview) => webview,
                                Err(error) => {
                                    unsafe {
                                        let _ = controller.Close();
                                    }
                                    return Err(error);
                                }
                            };
                            let mut state = state.borrow_mut();
                            state.controller = Some(controller);
                            state.webview = Some(webview);
                            Ok(())
                        })();
                        if let Err(error) = initialized {
                            state.borrow_mut().fail(format!("{error:#}"));
                        }
                        Ok(())
                    },
                ));
                unsafe {
                    environment.CreateCoreWebView2Controller(parent, &handler)?;
                }
                Ok(())
            })();
            if let Err(error) = initialized {
                state.borrow_mut().fail(format!("{error:#}"));
            }
            Ok(())
        },
    ));
    let options = CoreWebView2EnvironmentOptions::default();
    unsafe {
        options.set_additional_browser_arguments("--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows".into());
        options.set_language("en-US".into());
    }
    let options: ICoreWebView2EnvironmentOptions = options.into();
    unsafe {
        create(
            PCWSTR::null(),
            PCWSTR(data_path.as_ptr()),
            options.as_raw(),
            handler.as_raw(),
        )
        .ok()?;
    }
    Ok(())
}
fn configure(
    controller: &ICoreWebView2Controller,
    webview: &ICoreWebView2,
    provider: Provider,
    parent: HWND,
    state: &Rc<RefCell<ViewState>>,
) -> Result<()> {
    unsafe {
        let settings = webview.Settings()?;
        settings.SetAreHostObjectsAllowed(false)?;
        settings.SetIsWebMessageEnabled(false)?;
        settings.SetAreDevToolsEnabled(false)?;
        let mut token = 0;
        controller.add_MoveFocusRequested(
            &MoveFocusRequestedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                    let _ = SetFocus(Some(parent));
                }
                Ok(())
            })),
            &mut token,
        )?;
        controller.add_AcceleratorKeyPressed(
            &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut key = 0;
                    let mut kind = COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN;
                    args.VirtualKey(&mut key)?;
                    args.KeyEventKind(&mut kind)?;
                    if key == u32::from(VK_F6.0) && kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN {
                        args.SetHandled(true)?;
                        let _ = SetFocus(Some(parent));
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let starting_state = Rc::downgrade(state);
        webview.add_NavigationStarting(
            &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                if let Some(args) = args {
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = CoTaskMemPWSTR::from(uri).to_string();
                    if !provider.allows(&uri) {
                        args.SetCancel(true)?;
                    } else if let Some(state) = starting_state.upgrade() {
                        let mut navigation = 0;
                        args.NavigationId(&mut navigation)?;
                        let mut state = state.borrow_mut();
                        if !state.closed && state.error.is_none() {
                            state.loading.begin(navigation, Instant::now());
                        }
                    }
                }
                Ok(())
            })),
            &mut token,
        )?;
        let completed_state = Rc::downgrade(state);
        webview.add_NavigationCompleted(
            &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                let (Some(state), Some(args)) = (completed_state.upgrade(), args) else {
                    return Ok(());
                };
                let mut navigation = 0;
                let mut success = BOOL::default();
                let mut error = COREWEBVIEW2_WEB_ERROR_STATUS_UNKNOWN;
                args.NavigationId(&mut navigation)?;
                args.IsSuccess(&mut success)?;
                args.WebErrorStatus(&mut error)?;
                let mut state = state.borrow_mut();
                if !state.closed && state.error.is_none()
                    && state.loading.complete(navigation, success.as_bool())
                    && !success.as_bool()
                {
                    state.fail(format!("{} could not load: {}. Check your connection, then close and reload this panel.", provider.name(), navigation_error(error)));
                }
                Ok(())
            })),
            &mut token,
        )?;
        let failed_state = Rc::downgrade(state);
        webview.add_ProcessFailed(
            &ProcessFailedEventHandler::create(Box::new(move |_, args| {
                let Some(state) = failed_state.upgrade() else {
                    return Ok(());
                };
                let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND_UNKNOWN_PROCESS_EXITED;
                if let Some(args) = args {
                    args.ProcessFailedKind(&mut kind)?;
                }
                state.borrow_mut().fail(format!(
                    "{} browser failed: {}. Close and reload this panel to retry.",
                    provider.name(),
                    process_error(kind)
                ));
                Ok(())
            })),
            &mut token,
        )?;
        webview.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetHandled(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        webview.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let download_control: ICoreWebView2_4 = webview.cast()?;
        download_control.add_DownloadStarting(
            &DownloadStartingEventHandler::create(Box::new(|_, args| {
                if let Some(args) = args {
                    args.SetCancel(true)?;
                }
                Ok(())
            })),
            &mut token,
        )?;

        controller.SetIsVisible(false)?;
        controller.SetBounds(RECT {
            left: 0,
            top: 0,
            right: 1600,
            bottom: 1000,
        })?;
        controller.SetZoomFactor(1.0)?;
        let url: Vec<u16> = provider.url().encode_utf16().chain(Some(0)).collect();
        webview.Navigate(PCWSTR(url.as_ptr()))?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loading_waits_for_current_navigation_and_redirects_do_not_extend_timeout() {
        let start = Instant::now();
        let mut loading = Loading {
            since: start,
            ..Loading::default()
        };
        assert!(!loading.ready);
        assert!(loading.timed_out(start + LOAD_TIMEOUT));

        loading.begin(7, start + Duration::from_secs(10));
        loading.begin(7, start + Duration::from_secs(40)); // Same-ID redirect.
        assert!(!loading.complete(6, true)); // Stale completion.
        assert!(!loading.ready);
        assert!(loading.timed_out(start + Duration::from_secs(55)));
        assert!(loading.complete(7, true));
        assert!(loading.ready);
        assert!(!loading.timed_out(start + Duration::from_secs(90)));

        loading.begin(8, start + Duration::from_secs(100));
        assert!(!loading.ready);
        assert!(!loading.complete(7, true));
        assert!(loading.complete(8, false));
        assert!(!loading.ready);
    }
    #[test]
    fn first_failure_is_preserved_and_closed_views_ignore_late_failure() {
        let mut state = ViewState::default();
        state.loading.ready = true;
        state.fail("DNS failed".into());
        state.fail("Renderer exited".into());
        assert_eq!(state.error.as_deref(), Some("DNS failed"));
        assert!(!state.loading.ready);
        let mut closed = ViewState {
            closed: true,
            ..ViewState::default()
        };
        closed.fail("Late callback".into());
        assert!(closed.error.is_none());
    }
    #[test]
    fn provider_navigation_checks_scheme_host_and_path() {
        for provider in PROVIDERS {
            assert!(provider.allows(provider.url()));
            for bad in [
                "http://wifiman.com/",
                "https://wifiman.com.evil.example/",
                "https://evil.example/?https://fast.com/",
                "file:///C:/Windows/win.ini",
                "https://user:secret@fast.com/",
                "https://fast.com:8443/",
            ] {
                assert!(!provider.allows(bad), "{provider:?} allowed {bad}");
            }
        }
        assert!(Provider::Fast.allows("https://fast.com/en/"));
        assert!(Provider::Speedtest.allows("https://www.speedtest.net/result/123"));
        assert!(Provider::Devina.allows("https://devina.io/speed-test?mode=bufferbloat"));
        assert!(!Provider::Devina.allows("https://devina.io/screen-recorder"));
        assert!(!Provider::WiFiman.allows("https://www.speedtest.net/"));
    }
    #[test]
    fn parses_only_the_observed_complete_layout() {
        let s = "Test Result\nInternet\nThis device\n195.93Mbps\n452.26Mbps\nPing: 17 ms\nTest Server\nUbiquiti server in Dallas";
        let c = parse_result(s).unwrap();
        assert_eq!((c.download, c.upload, c.ping), (195.93, 452.26, 17.));
        assert!(parse_result("Download\n195.93Mbps").is_err());
        assert!(parse_result(&s.replace("452.26Mbps", "unknown")).is_err());
    }
    #[test]
    fn normalizes_gigabits() {
        let s = "Test Result\nInternet\nThis device\n1.23Gbps\n100Mbps\nPing: 2.5 ms";
        assert_eq!(parse_result(s).unwrap().download, 1230.);
    }
}
