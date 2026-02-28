use std::time::Instant;
use tauri::{webview::PageLoadEvent, Manager, RunEvent, WindowEvent};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_updater::UpdaterExt;

use crate::{
    append_desktop_log, append_startup_log, desktop_bridge, exit_events, startup_loading,
    startup_task, tray_setup, window_actions, AutoUpdateCheckState, BackendState,
    DEFAULT_SHELL_LOCALE, DESKTOP_LOG_FILE, STARTUP_MODE_ENV,
};

pub(crate) fn run() {
    let packaged_root_dir = crate::runtime_paths::default_packaged_root_dir();
    let auto_update_check_enabled =
        crate::shell_locale::read_cached_auto_update_check_enabled(packaged_root_dir.as_deref())
            .unwrap_or(true);

    append_startup_log("desktop process starting");
    append_startup_log(&format!(
        "desktop log path: {}",
        crate::logging::resolve_desktop_log_path(
            crate::runtime_paths::default_packaged_root_dir(),
            DESKTOP_LOG_FILE,
        )
        .display()
    ));
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(BackendState::default())
        .manage(AutoUpdateCheckState::new(auto_update_check_enabled))
        .invoke_handler(tauri::generate_handler![
            crate::desktop_bridge_commands::desktop_bridge_is_desktop_runtime,
            crate::desktop_bridge_commands::desktop_bridge_get_backend_state,
            crate::desktop_bridge_commands::desktop_bridge_set_auth_token,
            crate::desktop_bridge_commands::desktop_bridge_set_shell_locale,
            crate::desktop_bridge_commands::desktop_bridge_restart_backend,
            crate::desktop_bridge_commands::desktop_bridge_stop_backend,
            crate::desktop_bridge_commands::desktop_bridge_open_external_url,
            crate::desktop_bridge_commands::desktop_bridge_check_desktop_app_update,
            crate::desktop_bridge_commands::desktop_bridge_install_desktop_app_update,
        ])
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }

            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    let app_handle = window.app_handle();
                    let state = app_handle.state::<BackendState>();
                    if state.is_quitting() {
                        return;
                    }

                    api.prevent_close();
                    window_actions::hide_main_window(
                        app_handle,
                        DEFAULT_SHELL_LOCALE,
                        append_desktop_log,
                    );
                }
                WindowEvent::Focused(false) => {
                    if let Ok(true) = window.is_minimized() {
                        let app_handle = window.app_handle();
                        let state = app_handle.state::<BackendState>();
                        if !state.is_quitting() {
                            window_actions::hide_main_window(
                                app_handle,
                                DEFAULT_SHELL_LOCALE,
                                append_desktop_log,
                            );
                        }
                    }
                }
                _ => {}
            }
        })
        .on_page_load(|webview, payload| match payload.event() {
            PageLoadEvent::Started => {
                append_desktop_log(&format!("page-load started: {}", payload.url()));
                let state = webview.app_handle().state::<BackendState>();
                if desktop_bridge::should_inject_desktop_bridge(&state.backend_url, payload.url()) {
                    crate::inject_desktop_bridge(webview);
                }
            }
            PageLoadEvent::Finished => {
                append_desktop_log(&format!("page-load finished: {}", payload.url()));
                let state = webview.app_handle().state::<BackendState>();
                if desktop_bridge::should_inject_desktop_bridge(&state.backend_url, payload.url()) {
                    crate::inject_desktop_bridge(webview);
                } else if startup_loading::should_apply_startup_loading_mode(
                    webview.window().label(),
                    payload.url(),
                ) {
                    startup_loading::apply_startup_loading_mode(
                        webview.app_handle(),
                        webview,
                        STARTUP_MODE_ENV,
                        append_startup_log,
                    );
                }
            }
        })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            if let Err(error) = tray_setup::setup_tray(&app_handle) {
                append_startup_log(&format!("failed to initialize tray: {error}"));
            }

            startup_task::spawn_startup_task(app_handle.clone(), append_startup_log);

            // 启动时静默检查更新；若发现新版本则弹窗询问是否立即下载并安装
            let startup_app_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let current_version = startup_app_handle.package_info().version.to_string();
                let auto_update_enabled = startup_app_handle
                    .try_state::<AutoUpdateCheckState>()
                    .map(|state| state.is_enabled())
                    .unwrap_or(true);
                if !auto_update_enabled {
                    append_startup_log("[更新检查] 自动检查更新已关闭，跳过本次检查");
                    return;
                }

                append_startup_log("[更新检查] 正在初始化更新器...");
                match startup_app_handle.updater() {
                    Ok(updater) => {
                        append_startup_log(&format!(
                            "[更新检查] 更新器初始化成功，正在检查更新... current_version={}",
                            current_version
                        ));
                        let check_started = Instant::now();
                        match updater.check().await {
                            Ok(Some(update)) => {
                                let new_version = update.version.to_string();
                                append_startup_log(&format!(
                                    "[更新检查] 检查完成：has_update=true current_version={} latest_version={} elapsed_ms={}",
                                    current_version,
                                    new_version,
                                    check_started.elapsed().as_millis()
                                ));

                                let dialog = startup_app_handle.dialog();
                                let should_update = dialog
                                    .message(format!(
                                        "发现新版本 {}，是否立即下载并安装？\n选择“否”可稍后手动更新。",
                                        new_version
                                    ))
                                    .title("发现新版本")
                                    .kind(tauri_plugin_dialog::MessageDialogKind::Info)
                                    .buttons(tauri_plugin_dialog::MessageDialogButtons::YesNo)
                                    .blocking_show();
                                append_startup_log(&format!(
                                    "[更新检查] 更新确认弹窗结果：{}",
                                    if should_update { "立即更新" } else { "稍后处理" }
                                ));

                                if !should_update {
                                    append_startup_log("[更新检查] 用户选择稍后处理更新");
                                    return;
                                }

                                append_startup_log("[更新检查] 用户确认更新，正在下载更新...");
                                let downloaded_bytes = match update.download(|_, _| {}, || {}).await
                                {
                                    Ok(bytes) => bytes,
                                    Err(error) => {
                                        append_startup_log(&format!(
                                            "[更新检查] 下载更新失败：{error}"
                                        ));
                                        return;
                                    }
                                };

                                append_startup_log(&format!(
                                    "[更新检查] 更新 {} 下载完成，正在安装",
                                    new_version
                                ));
                                if let Err(error) = update.install(&downloaded_bytes) {
                                    append_startup_log(&format!(
                                        "[更新检查] 安装更新失败：{error}"
                                    ));
                                    return;
                                }

                                append_startup_log(&format!(
                                    "[更新检查] 更新 {} 安装完成，正在重启应用",
                                    new_version
                                ));
                                startup_app_handle.request_restart();
                            }
                            Ok(None) => {
                                append_startup_log(&format!(
                                    "[更新检查] 检查完成：has_update=false current_version={} latest_version={} elapsed_ms={}",
                                    current_version,
                                    current_version,
                                    check_started.elapsed().as_millis()
                                ));
                            }
                            Err(error) => {
                                // 静默处理错误，只记录到日志，不显示给用户
                                // 首次安装或 latest.json 不存在时会触发此错误，属于正常情况
                                append_startup_log(&format!(
                                    "[更新检查] 检查失败（静默）：current_version={} elapsed_ms={} error={}",
                                    current_version,
                                    check_started.elapsed().as_millis(),
                                    error
                                ));
                            }
                        }
                    }
                    Err(error) => {
                        append_startup_log(&format!("[更新检查] 初始化更新器失败：{error}"));
                    }
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            RunEvent::ExitRequested { api, .. } => {
                exit_events::handle_exit_requested(app_handle, &api);
            }
            RunEvent::Exit => {
                exit_events::handle_exit_event(app_handle);
            }
            _ => {}
        });
}
