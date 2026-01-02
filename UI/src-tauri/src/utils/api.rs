use std::{ffi::OsStr, os::windows::ffi::OsStrExt};

use crate::utils::custom_result::CustomResult;
use serde_json::json;
use windows::{
    core::{HRESULT, HSTRING, PWSTR},
    Win32::{
        Foundation::{CloseHandle, GetLastError, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{
            CreateFileW, WriteFile, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, OPEN_EXISTING,
        },
        System::{
            Pipes::WaitNamedPipeW, Shutdown::LockWorkStation, WindowsProgramming::GetUserNameW,
        },
    },
};

// 获取当前用户名
#[tauri::command]
pub fn get_now_username() -> Result<CustomResult, CustomResult> {
    // buffer大小，256应该够了
    let mut buffer = [0u16; 256];
    let mut size = buffer.len() as u32;
    unsafe {
        let succuess = GetUserNameW(Some(PWSTR(buffer.as_mut_ptr())), &mut size);
        if succuess.is_err() {
            return Err(CustomResult::error(
                Some(format!("获取用户名失败: {:?}", succuess.err())),
                None,
            ));
        }

        let name = String::from_utf16_lossy(&buffer[..size as usize - 1]);
        return Ok(CustomResult::success(None, Some(json!({"username": name}))));
    }
}

// 测试 WinLogon 是否加载成功
#[tauri::command]
pub fn test_win_logon(user_name: String, password: String) -> Result<CustomResult, CustomResult> {
    // 锁定屏幕
    unsafe {
        let succuess = LockWorkStation();
        if succuess.is_err() {
            return Err(CustomResult::error(
                Some(format!("锁定屏幕失败: {:?}", succuess.err())),
                None,
            ));
        }

        // 等待5秒
        std::thread::sleep(std::time::Duration::from_secs(5));
        // 解锁
        unlock(user_name, password).map_err(|e| CustomResult::error(Some(format!("解锁屏幕失败: {:?}", e)), None))?;
    }
    return Ok(CustomResult::success(None, None));
}

// 解锁屏幕
pub fn unlock(user_name: String, password: String) -> windows::core::Result<()> {
    unsafe {
        let pipe_name = HSTRING::from("\\\\.\\pipe\\MansonWindowsUnlockRust");
        // 等待管道连接
        if !WaitNamedPipeW(&pipe_name.clone(), 5000).as_bool() {
            return Err(windows::core::Error::new(
                HRESULT(0),
                "不能连接到管道: MansonWindowsUnlockRust",
            ));
        }

        // 打开管道
        let handle = CreateFileW(
            &pipe_name.clone(), // 管道名称
            GENERIC_WRITE.0,    // 对文件的操作模式，只写
            FILE_SHARE_MODE(0), // 阻止对管道的后续打开操作，在我主动关闭之前
            None,
            OPEN_EXISTING, // 只在文件存在时才打开，否则返回错误
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        );
        if handle.is_err() {
            return Err(windows::core::Error::new(
                HRESULT(0),
                format!("打开管道失败: {:?}", handle.err()),
            ));
        }
        let handle = handle.unwrap();

        // 向管道发送用户名
        let write_success = send_to_pipe(user_name, handle);
        if write_success.is_err() {
            let _ = CloseHandle(handle);
            return Err(windows::core::Error::new(
                HRESULT(0),
                format!(
                    "发送用户名失败: {:?}, 扩展信息: {:?}",
                    write_success.err(),
                    GetLastError()
                ),
            ));
        }

        // 向管道发送密码
        let write_success = send_to_pipe(password, handle);
        if write_success.is_err() {
            let _ = CloseHandle(handle);
            return Err(windows::core::Error::new(
                HRESULT(0),
                format!(
                    "发送密码失败: {:?}, 扩展信息: {:?}",
                    write_success.err(),
                    GetLastError()
                ),
            ));
        }

        let _ = CloseHandle(handle);
    };

    Ok(())
}

// 向管道发送数据
fn send_to_pipe(content: String, handle: HANDLE) -> windows::core::Result<()> {
    unsafe {
        // 转 UTF-16 含 \0
        let wide_chars: Vec<u16> = OsStr::new(&content)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // 转 &[u8] 切片
        let write_buf =
            std::slice::from_raw_parts(wide_chars.as_ptr() as *const u8, wide_chars.len() * 2);
        // 准备字节数
        let mut total_bytes = write_buf.len() as u32;

        WriteFile(handle, Some(write_buf), Some(&mut total_bytes), None)
    }
}
