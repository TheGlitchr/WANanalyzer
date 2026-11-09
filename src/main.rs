#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use anyhow::Result;
use std::{path::PathBuf, time::Duration};
use wanalyzer::{
    engine::Monitor,
    model::*,
    storage::{Store, load_settings},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        if std::env::args().any(|arg| arg == "--headless-seconds") {
            std::process::exit(1);
        }
        let _ = rfd::MessageDialog::new()
            .set_title("WANanalyzer")
            .set_level(rfd::MessageLevel::Error)
            .set_description(format!("{error:#}"))
            .show();
    }
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    let arg = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|p| args.get(p + 1))
            .cloned()
    };
    if args.iter().any(|arg| arg == "--speed-check") {
        anyhow::ensure!(
            arg("--speed-check")
                .as_deref()
                .and_then(wanalyzer::speed::Provider::from_key)
                .is_some(),
            "Use --speed-check wifiman, speedtest, fast, devina, or iperf"
        );
    }
    let mut directory = arg("--data-dir").map(PathBuf::from).unwrap_or_else(|| {
        std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .join("data")
    });
    std::fs::create_dir_all(&directory)?;
    directory = std::fs::canonicalize(directory)?;
    // Per-data-directory instance mutex prevents duplicate sessions and DB writers.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    directory.to_string_lossy().to_lowercase().hash(&mut hasher);
    let mutex_name: Vec<u16> = format!("Local\\WANanalyzer-{:x}", hasher.finish())
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mutex = unsafe {
        windows_sys::Win32::System::Threading::CreateMutexW(
            std::ptr::null(),
            0,
            mutex_name.as_ptr(),
        )
    };
    anyhow::ensure!(!mutex.is_null(), "Cannot create instance mutex");
    if unsafe { windows_sys::Win32::Foundation::GetLastError() } == 183 {
        let title: Vec<u16> = "WANanalyzer\0".encode_utf16().collect();
        unsafe {
            let hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                std::ptr::null(),
                title.as_ptr(),
            );
            if !hwnd.is_null() {
                windows_sys::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, 9);
                windows_sys::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
            }
            windows_sys::Win32::Foundation::CloseHandle(mutex);
        }
        return Ok(());
    }
    let session = uuid::Uuid::new_v4().to_string();
    let store = Store::open(&directory, &session)?;
    let (settings, startup_notice) = match load_settings(&directory) {
        Ok(settings) => (settings, None),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            (Settings::default(), None)
        }
        Err(error) => (
            Settings::default(),
            Some(format!(
                "Saved settings could not be loaded; using defaults for this session. Your file has been preserved. Open Settings to review and save: {error}"
            )),
        ),
    };
    let mut monitor = Monitor::start(store.clone(), session.clone(), settings.clone());
    if let Some(seconds) = arg("--headless-seconds") {
        let seconds: u64 = seconds.parse()?;
        std::thread::sleep(Duration::from_secs(seconds));
        monitor.stop();
        store.flush().ok();
        let state = monitor.shared.lock().unwrap();
        let targets:Vec<_>=(0..4).map(|i|{let mut stats=Stats::default();let endpoint:Vec<_>=state.samples.iter().filter(|p|p.target==i&&p.ttl.is_none()).collect();for sample in endpoint{stats.add(sample);}
            let mut hops=std::collections::BTreeMap::new();for ttl in 1..=settings.max_hops{let mut s=Stats::default();for p in state.samples.iter().filter(|p|p.target==i&&p.ttl==Some(ttl)){s.add(p);}if s.sent+s.local_errors>0{hops.insert(ttl,serde_json::json!({"sent":s.sent,"replies":s.responses,"lost":s.lost,"loss_pct":s.loss(),"local_errors":s.local_errors,"responders":s.responders}));}}
            serde_json::json!({"target":TARGET_HOSTS[i],"network":state.targets[i].network,"endpoint_sent":stats.sent,"endpoint_loss_pct":stats.loss(),"hops":hops})}).collect();
        std::fs::write(
            directory.join("diagnostics.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"session":session,"submitted":state.submitted,"samples":state.samples.len(),"targets":targets}),
            )?,
        )?;
    } else {
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/wanalyzer.png"))?;
        let options = eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_icon(icon)
                .with_inner_size([1000., 700.])
                .with_maximized(true)
                .with_min_inner_size([900., 650.]),
            renderer: eframe::Renderer::Glow,
            ..Default::default()
        };
        let smoke = arg("--smoke-ui-seconds").and_then(|s| s.parse().ok());
        eframe::run_native(
            "WANanalyzer",
            options,
            Box::new(move |cc| {
                Ok(Box::new(wanalyzer::ui::App::new(
                    cc,
                    monitor,
                    store,
                    directory,
                    settings,
                    smoke,
                    startup_notice,
                )))
            }),
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(mutex);
    }
    Ok(())
}
