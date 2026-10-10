use std::{
    process::{Command, ExitCode},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    routing::post,
    Router,
};
use tauri::{
    async_runtime::JoinHandle, webview::Color, AppHandle, Manager, RunEvent, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};
use tokio_util::sync::CancellationToken;

use cursor_server::config::ConsoleSource;
use cursor_server::{App, Config, Result};

#[cfg(not(dev))]
use crate::frontend;
use crate::startup::{self, StartupDiagnostics};
use crate::tray;

pub(crate) const MAIN_WINDOW_LABEL: &str = "main";
const AUTOSTART_ARG: &str = "--autostart";
const WINDOW_STATE_FLAGS: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);

struct DesktopRuntime {
    shutdown: CancellationToken,
    server: Mutex<Option<JoinHandle<Result<()>>>>,
    exiting: AtomicBool,
    server_addr: std::net::SocketAddr,
}

#[tauri::command]
fn open_terminal_with_command(command: String) -> tauri::Result<()> {
    use std::os::windows::process::CommandExt;
    Command::new("cmd").args(["/K"]).raw_arg(&command).spawn()?;
    Ok(())
}

#[tauri::command]
fn is_headless_service_installed() -> bool {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(run_key) = hkcu.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_READ) {
            return run_key.get_value::<String, _>("NexusorHeadlessService").is_ok();
        }
    }
    false
}

#[tauri::command]
fn set_headless_service(app: AppHandle, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (run_key, _) = hkcu
            .create_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_SET_VALUE)
            .map_err(|e| format!("registry access error: {e}"))?;

        if enabled {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let target_cmd = format!("\"{}\" --silent", exe.to_string_lossy());
            run_key
                .set_value("NexusorHeadlessService", &target_cmd)
                .map_err(|e| format!("failed to register startup key: {e}"))?;

            // Tek bir exe olarak çalışır: Tray simgesini gizle ve UI penceresini gizle.
            // Arka plandaki HTTP proxy sunucusu tek exe içinde kesintisiz çalışmaya devam eder.
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_visible(false);
            }
            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = window.hide();
            }
        } else {
            let _ = run_key.delete_value("NexusorHeadlessService");
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_visible(true);
            }
        }
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        let _ = enabled;
        Ok(())
    }
}

#[derive(serde::Deserialize)]
struct OpenExternalUrlRequest {
    url: String,
}

fn open_external_url(app: &AppHandle, url: &str) -> std::result::Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err("only absolute HTTP and HTTPS URLs are allowed".into());
    }

    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|error| format!("failed to open URL: {error}"))
}

async fn open_external_url_handler(
    Extension(app): Extension<AppHandle>,
    Json(request): Json<OpenExternalUrlRequest>,
) -> std::result::Result<StatusCode, (StatusCode, String)> {
    open_external_url(&app, &request.url)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|error| (StatusCode::BAD_REQUEST, error))
}

fn desktop_api_router(app: AppHandle) -> Router {
    Router::new()
        .route(
            "/__byok-api__/api/desktop/open-external-url",
            post(open_external_url_handler),
        )
        .layer(Extension(app))
}

fn default_window_size(app: &AppHandle) -> (f64, f64) {
    let monitor = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| app.available_monitors().ok().into_iter().flatten().next());
    if let Some(monitor) = monitor {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let screen_w = size.width as f64 / scale;
        let screen_h = size.height as f64 / scale;
        (
            (screen_w * 0.5).clamp(900.0, screen_w),
            (screen_h * 0.5).clamp(600.0, screen_h),
        )
    } else {
        (1180.0, 760.0)
    }
}

fn create_main_window(
    app: &AppHandle,
    address: std::net::SocketAddr,
) -> tauri::Result<WebviewWindow> {
    let url = format!("http://{address}/__byok-api__/")
        .parse()
        .expect("local frontend URL");
    let (width, height) = default_window_size(app);
    let window = WebviewWindowBuilder::new(app, MAIN_WINDOW_LABEL, WebviewUrl::External(url))
        .title("Nexusor")
        .inner_size(width, height)
        .min_inner_size(900.0, 600.0)
        .center()
        .background_color(Color(20, 20, 20, 255))
        .decorations(false)
        .shadow(true)
        .resizable(true)
        .visible(false)
        .build()?;

    // Tray mode keeps the process alive after the window closes; persist size/position
    // immediately so the next open restores it without waiting for full app Exit.
    let app_handle = app.clone();
    window.on_window_event(move |event| {
        if matches!(
            event,
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
        ) {
            let _ = app_handle.save_window_state(WINDOW_STATE_FLAGS);
        }
    });

    Ok(window)
}

/// 按需打开主窗口:webview 仅在需要界面时创建,关闭窗口即销毁释放内存。
pub(crate) fn open_main_window(app: &AppHandle) -> tauri::Result<()> {
    tracing::info!("open_main_window invoked");
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        tracing::info!("open_main_window found existing window; showing and focusing");
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }
    let address = match app.try_state::<DesktopRuntime>() {
        Some(runtime) => runtime.server_addr,
        None => {
            tracing::error!("DesktopRuntime state missing in open_main_window");
            return Ok(());
        }
    };
    match create_main_window(app, address) {
        Ok(window) => {
            let _ = window.show();
            let _ = window.set_focus();
            tracing::info!("open_main_window successfully created and showed webview window");
            Ok(())
        }
        Err(err) => {
            tracing::error!(%err, "open_main_window failed to create window");
            Err(err)
        }
    }
}

pub fn run() -> ExitCode {
    let diagnostics = match StartupDiagnostics::initialize() {
        Ok(diagnostics) => diagnostics,
        Err(error) => {
            startup::report_logging_failure(error.as_ref());
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        architecture = std::env::consts::ARCH,
        log_directory = %diagnostics.log_directory().display(),
        "desktop starting"
    );

    let started_by_autostart = std::env::args_os().any(|arg| arg == AUTOSTART_ARG);

    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_terminal_with_command,
            is_headless_service_installed,
            set_headless_service
        ])
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            let started_silent = args.iter().any(|arg| arg == AUTOSTART_ARG || arg == "--silent" || arg == "-s");
            if !started_silent {
                let app_handle = app.clone();
                let _ = app.run_on_main_thread(move || {
                    let _ = open_main_window(&app_handle);
                    if let Some(tray) = app_handle.tray_by_id("main") {
                        let _ = tray.set_visible(true);
                    }
                });
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(WINDOW_STATE_FLAGS)
                .build(),
        )
        .setup(move |app| {
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                Some(vec![AUTOSTART_ARG]),
            ))?;
            let mut config = Config::desktop()?;
            // 插件的 minAppVersion 按桌面应用版本判定,而不是内嵌 server 库的版本。
            config.app_version = env!("CARGO_PKG_VERSION").into();
            // Only proxy to Vite if explicitly requested or if running in dev-specific build.
            // In normal debug/standalone execution, serve embedded frontend so it works standalone.
            if let Ok(proxy) = std::env::var("CURSOR_CONSOLE_PROXY") {
                let proxy = proxy
                    .parse()
                    .map_err(|error| format!("invalid CURSOR_CONSOLE_PROXY: {error}"))?;
                config.console = Some(ConsoleSource::Proxy(proxy));
            }
            #[cfg(dev)]
            {
                if config.console.is_none() {
                    config.console = Some(ConsoleSource::Proxy(
                        "http://127.0.0.1:1420"
                            .parse()
                            .expect("Vite development URL"),
                    ));
                }
            }
            let serve_embedded_frontend = config.console.is_none();
            tracing::info!(
                tauri_is_dev = tauri::is_dev(),
                debug_assertions = cfg!(debug_assertions),
                vite_proxy = !serve_embedded_frontend,
                embedded_frontend = serve_embedded_frontend,
                "desktop frontend source"
            );
            let server = tauri::async_runtime::block_on(App::new(config))?
                .merge_router(desktop_api_router(app.handle().clone()));
            #[cfg(not(dev))]
            let server = if serve_embedded_frontend {
                server.merge_router(frontend::router(app.handle().clone()))
            } else {
                server
            };
            #[cfg(dev)]
            let server = {
                let _ = serve_embedded_frontend;
                server
            };
            let listener = tauri::async_runtime::block_on(server.bind())?;
            let address = listener.local_addr()?;
            tauri::async_runtime::block_on(server.harness().cleanup_stale_settings())?;
            let desktop_settings =
                tauri::async_runtime::block_on(server.store().desktop_settings())
                    .unwrap_or_default();
            let shutdown = CancellationToken::new();
            let server_shutdown = shutdown.clone();
            let app_handle = app.handle().clone();
            let task = tauri::async_runtime::spawn(async move {
                let result = server.serve_on(listener, server_shutdown).await;
                if let Err(error) = &result {
                    tracing::error!(%error, "desktop server stopped unexpectedly");
                    app_handle.exit(1);
                }
                result
            });
            app.manage(DesktopRuntime {
                shutdown,
                server: Mutex::new(Some(task)),
                exiting: AtomicBool::new(false),
                server_addr: address,
            });
            let started_silent = started_by_autostart
                || std::env::args().any(|arg| arg == "--silent" || arg == "-s");
            // Her durumda pencereyi hazır oluştur (arka planda gizli dursun), böylece tekrar tıklandığında anında ekrana gelir
            open_main_window(app.handle())?;
            if (desktop_settings.silent_start && started_by_autostart) || started_silent {
                tracing::info!("silent start requested; hiding the main window");
                if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                    let _ = window.hide();
                }
            }
            tray::create(app)?;
            if started_silent {
                if let Some(tray) = app.tray_by_id("main") {
                    let _ = tray.set_visible(false);
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = match app {
        Ok(app) => app,
        Err(error) => {
            diagnostics.report_fatal(&error);
            return ExitCode::FAILURE;
        }
    };

    app.run(|app, event| {
        // code 为 None 表示所有窗口已被关闭(轻量模式),阻止退出,
        // 转发服务继续在托盘后台运行;code 为 Some 时是显式退出请求。
        if let RunEvent::ExitRequested { code, api, .. } = event {
            match code {
                None => api.prevent_exit(),
                Some(_) => {
                    let runtime = app.state::<DesktopRuntime>();
                    if !runtime.exiting.swap(true, Ordering::AcqRel) {
                        api.prevent_exit();
                        runtime.shutdown.cancel();
                        let server = runtime.server.lock().expect("server lock poisoned").take();
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Some(server) = server {
                                match tokio::time::timeout(Duration::from_secs(11), server).await {
                                    Ok(Ok(Ok(()))) => {}
                                    Ok(Ok(Err(error))) => {
                                        tracing::error!(%error, "desktop server shutdown failed")
                                    }
                                    Ok(Err(error)) => {
                                        tracing::error!(%error, "desktop server task failed")
                                    }
                                    Err(_) => tracing::warn!("desktop server shutdown timed out"),
                                }
                            }
                            app.exit(0);
                        });
                    }
                }
            }
        }
    });

    ExitCode::SUCCESS
}
