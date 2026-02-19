#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Deserialize;
use std::{
    env,
    fs::{self, OpenOptions},
    net::{TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tauri::{path::BaseDirectory, AppHandle, Manager, RunEvent};
use url::Url;

const DEFAULT_BACKEND_URL: &str = "http://127.0.0.1:6185/";
const PACKAGED_BACKEND_TIMEOUT_FALLBACK_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Deserialize)]
struct RuntimeManifest {
    python: Option<String>,
    entrypoint: Option<String>,
}

#[derive(Debug)]
struct LaunchPlan {
    cmd: String,
    args: Vec<String>,
    cwd: PathBuf,
    root_dir: Option<PathBuf>,
    webui_dir: Option<PathBuf>,
    packaged_mode: bool,
}

#[derive(Debug)]
struct BackendState {
    child: Mutex<Option<Child>>,
    backend_url: String,
}

impl Default for BackendState {
    fn default() -> Self {
        Self {
            child: Mutex::new(None),
            backend_url: normalize_backend_url(
                &env::var("ASTRBOT_BACKEND_URL").unwrap_or_else(|_| DEFAULT_BACKEND_URL.to_string()),
            ),
        }
    }
}

impl BackendState {
    fn ensure_backend_ready(&self, app: &AppHandle) -> Result<(), String> {
        if self.ping_backend(800) {
            return Ok(());
        }

        if env::var("ASTRBOT_BACKEND_AUTO_START")
            .unwrap_or_else(|_| "1".to_string())
            == "0"
        {
            return Err("Backend auto-start is disabled (ASTRBOT_BACKEND_AUTO_START=0).".to_string());
        }

        let plan = self.resolve_launch_plan(app)?;
        self.start_backend_process(&plan)?;
        self.wait_for_backend(&plan)
    }

    fn resolve_launch_plan(&self, app: &AppHandle) -> Result<LaunchPlan, String> {
        if let Some(custom_cmd) = env::var("ASTRBOT_BACKEND_CMD")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            return self.resolve_custom_launch(custom_cmd);
        }

        if let Some(plan) = self.resolve_packaged_launch(app)? {
            return Ok(plan);
        }

        self.resolve_dev_launch()
    }

    fn resolve_custom_launch(&self, custom_cmd: String) -> Result<LaunchPlan, String> {
        let mut pieces = shlex::split(&custom_cmd)
            .ok_or_else(|| format!("Invalid ASTRBOT_BACKEND_CMD: {custom_cmd}"))?;
        if pieces.is_empty() {
            return Err("ASTRBOT_BACKEND_CMD is empty.".to_string());
        }

        let cmd = pieces.remove(0);
        let cwd = env::var("ASTRBOT_BACKEND_CWD")
            .map(PathBuf::from)
            .ok()
            .or_else(detect_astrbot_source_root)
            .unwrap_or_else(workspace_root_dir);
        let root_dir = env::var("ASTRBOT_ROOT").ok().map(PathBuf::from);
        let webui_dir = env::var("ASTRBOT_WEBUI_DIR").ok().map(PathBuf::from);

        Ok(LaunchPlan {
            cmd,
            args: pieces,
            cwd,
            root_dir,
            webui_dir,
            packaged_mode: false,
        })
    }

    fn resolve_packaged_launch(&self, app: &AppHandle) -> Result<Option<LaunchPlan>, String> {
        let manifest_path = match resolve_resource_path(app, "backend/runtime-manifest.json") {
            Some(path) if path.is_file() => path,
            _ => return Ok(None),
        };
        let backend_dir = manifest_path
            .parent()
            .ok_or_else(|| format!("Invalid backend manifest path: {}", manifest_path.display()))?;

        let manifest_text = fs::read_to_string(&manifest_path).map_err(|error| {
            format!(
                "Failed to read packaged backend manifest {}: {}",
                manifest_path.display(),
                error
            )
        })?;
        let manifest: RuntimeManifest = serde_json::from_str(&manifest_text).map_err(|error| {
            format!(
                "Failed to parse packaged backend manifest {}: {}",
                manifest_path.display(),
                error
            )
        })?;

        let default_python_relative = if cfg!(target_os = "windows") {
            PathBuf::from("python").join("Scripts").join("python.exe")
        } else {
            PathBuf::from("python").join("bin").join("python3")
        };
        let python_path = backend_dir.join(
            manifest
                .python
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or(default_python_relative),
        );
        if !python_path.is_file() {
            return Err(format!(
                "Packaged runtime python executable is missing: {}",
                python_path.display()
            ));
        }

        let entrypoint_relative = manifest
            .entrypoint
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("launch_backend.py"));
        let launch_script_path = backend_dir.join(entrypoint_relative);
        if !launch_script_path.is_file() {
            return Err(format!(
                "Packaged backend launch script is missing: {}",
                launch_script_path.display()
            ));
        }

        let root_dir = env::var("ASTRBOT_ROOT")
            .map(PathBuf::from)
            .ok()
            .or_else(default_packaged_root_dir);
        let cwd = env::var("ASTRBOT_BACKEND_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| root_dir.clone().unwrap_or_else(|| backend_dir.to_path_buf()));
        let webui_dir = env::var("ASTRBOT_WEBUI_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                resolve_resource_path(app, "webui/index.html")
                    .and_then(|index_path| index_path.parent().map(Path::to_path_buf))
            });

        let plan = LaunchPlan {
            cmd: python_path.to_string_lossy().to_string(),
            args: vec![launch_script_path.to_string_lossy().to_string()],
            cwd,
            root_dir,
            webui_dir,
            packaged_mode: true,
        };
        Ok(Some(plan))
    }

    fn resolve_dev_launch(&self) -> Result<LaunchPlan, String> {
        let source_root = detect_astrbot_source_root().ok_or_else(|| {
            "Cannot locate AstrBot source directory. Set ASTRBOT_SOURCE_DIR, or configure ASTRBOT_SOURCE_GIT_URL/ASTRBOT_SOURCE_GIT_REF and run resource prepare.".to_string()
        })?;

        let mut args = vec!["run".to_string(), "main.py".to_string()];
        let webui_dir = env::var("ASTRBOT_WEBUI_DIR")
            .ok()
            .map(PathBuf::from)
            .or_else(|| {
                let candidate = source_root.join("dashboard").join("dist");
                if candidate.join("index.html").is_file() {
                    Some(candidate)
                } else {
                    None
                }
            });
        if let Some(path) = &webui_dir {
            args.push("--webui-dir".to_string());
            args.push(path.to_string_lossy().to_string());
        }

        Ok(LaunchPlan {
            cmd: "uv".to_string(),
            args,
            cwd: env::var("ASTRBOT_BACKEND_CWD")
                .map(PathBuf::from)
                .unwrap_or(source_root),
            root_dir: env::var("ASTRBOT_ROOT").ok().map(PathBuf::from),
            webui_dir,
            packaged_mode: false,
        })
    }

    fn start_backend_process(&self, plan: &LaunchPlan) -> Result<(), String> {
        if self.child.lock().map_err(|_| "Backend process lock poisoned.")?.is_some() {
            return Ok(());
        }

        if !plan.cwd.exists() {
            fs::create_dir_all(&plan.cwd)
                .map_err(|error| format!("Failed to create backend cwd {}: {}", plan.cwd.display(), error))?;
        }
        if let Some(root_dir) = &plan.root_dir {
            if !root_dir.exists() {
                fs::create_dir_all(root_dir).map_err(|error| {
                    format!("Failed to create backend root directory {}: {}", root_dir.display(), error)
                })?;
            }
        }

        let mut command = Command::new(&plan.cmd);
        command
            .args(&plan.args)
            .current_dir(&plan.cwd)
            .stdin(Stdio::null())
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONUTF8", env::var("PYTHONUTF8").unwrap_or_else(|_| "1".to_string()))
            .env(
                "PYTHONIOENCODING",
                env::var("PYTHONIOENCODING").unwrap_or_else(|_| "utf-8".to_string()),
            );

        if plan.packaged_mode {
            command.env("ASTRBOT_ELECTRON_CLIENT", "1");
            if env::var("DASHBOARD_HOST").is_err() && env::var("ASTRBOT_DASHBOARD_HOST").is_err() {
                command.env("DASHBOARD_HOST", "127.0.0.1");
            }
            if env::var("DASHBOARD_PORT").is_err() && env::var("ASTRBOT_DASHBOARD_PORT").is_err() {
                command.env("DASHBOARD_PORT", "6185");
            }
        }

        if let Some(root_dir) = &plan.root_dir {
            command.env("ASTRBOT_ROOT", root_dir);
        }
        if let Some(webui_dir) = &plan.webui_dir {
            command.env("ASTRBOT_WEBUI_DIR", webui_dir);
        }

        if let Some(log_path) = backend_log_path(plan.root_dir.as_deref()) {
            if let Some(log_parent) = log_path.parent() {
                fs::create_dir_all(log_parent).map_err(|error| {
                    format!(
                        "Failed to create backend log directory {}: {}",
                        log_parent.display(),
                        error
                    )
                })?;
            }
            let stdout_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
                .map_err(|error| format!("Failed to open backend log {}: {}", log_path.display(), error))?;
            let stderr_file = stdout_file
                .try_clone()
                .map_err(|error| format!("Failed to clone backend log handle: {error}"))?;
            command.stdout(Stdio::from(stdout_file));
            command.stderr(Stdio::from(stderr_file));
        } else {
            command.stdout(Stdio::null());
            command.stderr(Stdio::null());
        }

        let child = command.spawn().map_err(|error| {
            format!(
                "Failed to spawn backend process with command {:?}: {}",
                build_debug_command(plan),
                error
            )
        })?;
        *self.child.lock().map_err(|_| "Backend process lock poisoned.")? = Some(child);
        Ok(())
    }

    fn wait_for_backend(&self, plan: &LaunchPlan) -> Result<(), String> {
        let timeout_ms = resolve_backend_timeout_ms(plan.packaged_mode);
        let start_time = Instant::now();

        loop {
            if self.ping_backend(800) {
                return Ok(());
            }

            {
                let mut guard = self
                    .child
                    .lock()
                    .map_err(|_| "Backend process lock poisoned.".to_string())?;
                if let Some(child) = guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            *guard = None;
                            return Err(format!(
                                "Backend process exited before becoming reachable: {status}"
                            ));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            return Err(format!("Failed to poll backend process status: {error}"));
                        }
                    }
                } else {
                    return Err("Backend process is not running.".to_string());
                }
            }

            if let Some(limit) = timeout_ms {
                if start_time.elapsed() >= limit {
                    return Err(format!(
                        "Timed out after {}ms waiting for backend startup.",
                        limit.as_millis()
                    ));
                }
            }

            thread::sleep(Duration::from_millis(600));
        }
    }

    fn ping_backend(&self, timeout_ms: u64) -> bool {
        let parsed = match Url::parse(&self.backend_url) {
            Ok(url) => url,
            Err(_) => return false,
        };
        let host = match parsed.host_str() {
            Some(host) => host.to_string(),
            None => return false,
        };
        let port = parsed.port_or_known_default().unwrap_or(80);
        let timeout = Duration::from_millis(timeout_ms.max(50));

        let addrs = match (host.as_str(), port).to_socket_addrs() {
            Ok(addrs) => addrs.collect::<Vec<_>>(),
            Err(_) => return false,
        };
        addrs
            .iter()
            .any(|address| TcpStream::connect_timeout(address, timeout).is_ok())
    }

    fn stop_backend(&self) {
        let mut child = match self.child.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some(process) = child.as_mut() {
            stop_child_process(process);
        }
    }
}

fn main() {
    tauri::Builder::default()
        .manage(BackendState::default())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let state = app_handle.state::<BackendState>();
            if let Err(error) = state.ensure_backend_ready(&app_handle) {
                show_startup_error(&app_handle, &error);
                return Ok(());
            }

            let Some(window) = app_handle.get_webview_window("main") else {
                show_startup_error(
                    &app_handle,
                    "Main window is unavailable after backend startup.",
                );
                return Ok(());
            };

            let js = format!(
                "window.location.replace({});",
                serde_json::to_string(&state.backend_url).unwrap_or_else(|_| "\"/\"".to_string())
            );
            if let Err(error) = window.eval(&js) {
                show_startup_error(
                    &app_handle,
                    &format!("Failed to navigate to backend dashboard: {error}"),
                );
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            RunEvent::ExitRequested { .. } | RunEvent::Exit => {
                let state = app_handle.state::<BackendState>();
                state.stop_backend();
            }
            _ => {}
        });
}

fn normalize_backend_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return DEFAULT_BACKEND_URL.to_string();
    }

    match Url::parse(trimmed) {
        Ok(mut parsed) => {
            if parsed.path().is_empty() {
                parsed.set_path("/");
            }
            parsed.to_string()
        }
        Err(_) => DEFAULT_BACKEND_URL.to_string(),
    }
}

fn workspace_root_dir() -> PathBuf {
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf())
}

fn detect_astrbot_source_root() -> Option<PathBuf> {
    if let Ok(source_dir) = env::var("ASTRBOT_SOURCE_DIR") {
        let candidate = PathBuf::from(source_dir.trim());
        if candidate.join("main.py").is_file() && candidate.join("astrbot").is_dir() {
            return Some(candidate.canonicalize().unwrap_or(candidate));
        }
    }

    let workspace_root = workspace_root_dir();
    let candidates = [
        workspace_root.join("vendor").join("AstrBot"),
        workspace_root.join("AstrBot"),
        workspace_root,
    ];
    for candidate in candidates {
        if candidate.join("main.py").is_file() && candidate.join("astrbot").is_dir() {
            return Some(candidate.canonicalize().unwrap_or(candidate));
        }
    }
    None
}

fn default_packaged_root_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".astrbot"))
}

fn resolve_backend_timeout_ms(packaged_mode: bool) -> Option<Duration> {
    let default_timeout_ms = if packaged_mode { 0_u64 } else { 20_000_u64 };
    let parsed_timeout_ms = env::var("ASTRBOT_BACKEND_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default_timeout_ms);

    if parsed_timeout_ms > 0 {
        return Some(Duration::from_millis(parsed_timeout_ms));
    }
    if packaged_mode {
        return Some(Duration::from_millis(PACKAGED_BACKEND_TIMEOUT_FALLBACK_MS));
    }
    None
}

fn backend_log_path(root_dir: Option<&Path>) -> Option<PathBuf> {
    root_dir.map(|root| root.join("logs").join("backend.log"))
}

fn stop_child_process(child: &mut Child) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("taskkill")
            .args(["/pid", &child.id().to_string(), "/t", "/f"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .status();
        let _ = child.wait();
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn build_debug_command(plan: &LaunchPlan) -> Vec<String> {
    let mut parts = vec![plan.cmd.clone()];
    parts.extend(plan.args.clone());
    parts
}

fn resolve_resource_path(app: &AppHandle, relative_path: &str) -> Option<PathBuf> {
    app.path()
        .resolve(relative_path, BaseDirectory::Resource)
        .ok()
}

fn show_startup_error(app_handle: &AppHandle, message: &str) {
    eprintln!("AstrBot startup failed: {message}");
    app_handle.exit(1);
}
