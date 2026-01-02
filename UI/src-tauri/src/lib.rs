// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use tauri::{Manager};
use windows::Win32::{Foundation::HWND, System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
},
UI::Shell::SetWindowSubclass
};

pub mod modules;
pub mod utils;
pub mod proc;
use modules::init::{check_admin_privileges, check_camera_status, deploy_core_components};
use utils::api::{get_now_username, test_win_logon};
use proc::wnd_proc_subclass;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .setup(|app| {
            let window = app.get_webview_window("main").unwrap();
            #[cfg(debug_assertions)] // 仅在调试(debug)版本中包含此代码
            {
                window.open_devtools();
                window.close_devtools();
            }

            #[cfg(windows)]
            {
                let window = app.get_webview_window("main").unwrap();
                let hwnd = window.hwnd().unwrap();
                unsafe {
                    // 注册 WTS 通知
                    let _ = WTSRegisterSessionNotification(HWND(hwnd.0), NOTIFY_FOR_THIS_SESSION);
    
                    // 注入子类化回调来捕获 WM_WTSSESSION_CHANGE
                    // on_window_event 收不到这个消息
                    let _ = SetWindowSubclass(HWND(hwnd.0), Some(wnd_proc_subclass), 0, 0);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" { 
                match event {
                    tauri::WindowEvent::CloseRequested{ .. } => {
                        let hwnd = window.hwnd().unwrap();
                        unsafe {
                            // 注销 WTS 通知
                            let _ = WTSUnRegisterSessionNotification(HWND(hwnd.0));
                        }
                    }
                    _ => {}
                }
                
            }
        })
        .invoke_handler(tauri::generate_handler![
            // init 初始化模块
            check_admin_privileges,
            check_camera_status,
            deploy_core_components,
            // 通用api
            get_now_username,
            test_win_logon
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
