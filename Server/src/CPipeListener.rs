use std::{os::raw::c_void, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, thread};
use windows::Win32::{
    Foundation::{CloseHandle, LocalFree, GENERIC_ALL, HLOCAL}, 
    Security::{
        AllocateAndInitializeSid, 
        Authorization::{SetEntriesInAclW, EXPLICIT_ACCESS_W, SET_ACCESS, TRUSTEE_IS_GROUP, TRUSTEE_IS_SID}, 
        InitializeSecurityDescriptor, SetSecurityDescriptorDacl, 
        ACL, 
        NO_INHERITANCE, 
        PSECURITY_DESCRIPTOR, 
        PSID, 
        SECURITY_ATTRIBUTES, 
        SECURITY_WORLD_SID_AUTHORITY, 
        SID_IDENTIFIER_AUTHORITY
    }, 
    Storage::FileSystem::{ReadFile, PIPE_ACCESS_INBOUND}, 
    System::{
        Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_TYPE_MESSAGE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT}, 
        SystemServices::SECURITY_DESCRIPTOR_REVISION
    }, UI::Shell::ICredentialProviderEvents
};
use windows_core::{BOOL, PWSTR};

use crate::SharedCredentials;

// 包装 COM 接口，使其可以跨线程传输
#[derive(Clone)]
struct SendableEvents(pub ICredentialProviderEvents);
// 声明这是安全的
unsafe impl Send for SendableEvents {}
unsafe impl Sync for SendableEvents {}

pub struct CPipeListener {
    pub is_unlocked: AtomicBool,
    pub running: Arc<AtomicBool>,
}

impl CPipeListener {
    pub fn start(provider_events: ICredentialProviderEvents, advise_context: usize, shared_creds_clone: Arc<Mutex<SharedCredentials>>) -> Arc<Self> {
        info!("CPipeListener::start - 启动管道监听");

        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        let listener = Arc::new(Self {
            is_unlocked: AtomicBool::new(false),
            running: running_clone.clone(),
        });

        let sendable_events = SendableEvents(provider_events);
        let listener_clone = listener.clone();

        thread::spawn(move || {
            info!("CPipeListener::start - 进入管道监听线程");
            let events_wrapper = sendable_events;
            let pipe_name = windows_core::w!(r"\\.\pipe\MansonWindowsUnlockRust");
            unsafe {
                while running_clone.load(Ordering::SeqCst) {
                    // 创建命名管道 
                    let h_pipe = CreateNamedPipeW(
                        pipe_name,
                        PIPE_ACCESS_INBOUND,
                        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
                        PIPE_UNLIMITED_INSTANCES,
                        512, 512, 0,
                        None // 先使用默认权限，如果创建失败，则使用 create_everyone_full_access_sa
                    );

                    if h_pipe.is_invalid() {
                        error!("创建管道失败");
                        break;
                    }

                    // 使命名管道服务器进程能够等待客户端进程连接到命名管道的实例
                    let f_connected = ConnectNamedPipe(
                        h_pipe, 
                        None // 创建管道时未指定FILE_FLAG_OVERLAPPED，使用同步模式，函数会阻塞线程，直到客户端连接成功或发生错误才返回
                    );

                    if f_connected.is_err() {
                        let _ = CloseHandle(h_pipe);
                        error!("管道连接失败：{:?}", f_connected.err());
                        break;
                    }

                    if !running_clone.load(Ordering::SeqCst) {
                        // 防止在退出时误读数据
                        let _ = CloseHandle(h_pipe);
                        break; 
                    }

                    let mut buf = [0u16; 256];
                    let mut read = 0;

                    // 获取 buf 的字节切片视图 (&mut [u8])
                    // buf 的字节长度是 256 * 2 (每个 u16 占两个字节)
                    let byte_slice = std::slice::from_raw_parts_mut(buf.as_mut_ptr() as *mut u8, buf.len() * 2);
                    
                    // 读取用户名
                    if ReadFile(h_pipe, Some(byte_slice), Some(&mut read), None).is_ok() {
                        let user = String::from_utf16_lossy(&buf[.. (read as usize / 2)]);
                        let mut creds = shared_creds_clone.lock().unwrap();
                        creds.username = user.trim_matches('\0').to_string();
                    }

                    // 读取密码前重置一下缓冲区
                    read = 0;

                    // 读取密码
                    if ReadFile(h_pipe, Some(byte_slice), Some(&mut read), None).is_ok() {
                        let pass = String::from_utf16_lossy(&buf[.. (read as usize / 2)]);
                        let mut creds = shared_creds_clone.lock().unwrap();
                        creds.password = pass.trim_matches('\0').to_string();
                    }

                    // 准备就绪
                    {
                        let mut creds = shared_creds_clone.lock().unwrap();
                        creds.is_ready = true;
                    }

                    running_clone.store(false, Ordering::SeqCst);
                    listener_clone.is_unlocked.store(true, Ordering::SeqCst);
                    
                    // 通知 UI 刷新，触发 GetCredentialCount
                    let _ = events_wrapper.0.CredentialsChanged(advise_context);
                    
                    let _ = CloseHandle(h_pipe);

                    break;
                }
            }
            info!("CPipeListener 线程已彻底退出");
        });
        listener
    }
}

impl Drop for CPipeListener {
    fn drop(&mut self) {
        info!("销毁一个 CPipeListener");
    }
}

// 创建安全标识符，授予 Everyone 组 GENERIC_ALL 权限的安全属性
fn create_everyone_full_access_sa() -> Result<(SECURITY_ATTRIBUTES, Vec<HLOCAL>), windows_core::Error> {
    // 用于存储需要手动释放的资源（避免内存泄漏）
    let mut resources = Vec::new();

    // 分配并初始化 Everyone SID
    let mut p_everyone_sid = PSID::default();
    // 安全标识符 (SID) 的顶级颁发机构
    let sid_auth_world = SID_IDENTIFIER_AUTHORITY {
        Value: SECURITY_WORLD_SID_AUTHORITY.Value,
    };

    unsafe {
        AllocateAndInitializeSid(
            &sid_auth_world,
            1, // 子授权数量
            0, 0, 0, 0, 0, 0, 0, 0, // 要放置在 SID 中的子授权值
            &mut p_everyone_sid, // 接收指向已分配和初始化 的 SID 结构的指针
        )?;

        // 记录需释放的 SID
        resources.push(HLOCAL(p_everyone_sid.0));
    }

    // 指定受托人的访问控制信息
    let mut ea = EXPLICIT_ACCESS_W::default();
    // 授予 GENERIC_ALL 权限
    ea.grfAccessPermissions = GENERIC_ALL.0; // 全部通用
    ea.grfAccessMode = SET_ACCESS; // 允许指定权限 的ACCESS_ALLOWED_ACE 结构
    ea.grfInheritance = NO_INHERITANCE; // 其他容器或对象 不可以 从 ACL 附加到的主对象继承 ACE
    // 指定 Trustee 为 Everyone SID（组）
    ea.Trustee.TrusteeForm = TRUSTEE_IS_SID; // ptstrName 成员是指向标识受托人的安全标识符 (SID) 的指针
    ea.Trustee.TrusteeType = TRUSTEE_IS_GROUP; // 受托人是一个组的类型
    ea.Trustee.ptstrName = PWSTR(p_everyone_sid.0 as *mut u16); // 	指向受托人的 SID 的指针

    // 创建 ACL
    let mut p_acl: *mut ACL = std::ptr::null_mut();
    unsafe {
        // 将新的访问控制或审核控制信息合并到现有的 ACL 结构来创建新的 访问控制列表
        let success = SetEntriesInAclW(
            Some(&[ea]), // 指向 EXPLICIT_ACCESS 结构的数组的指针
            None, // 无现有 ACL
            &mut p_acl, // 指向接收指向新 ACL 的指针的变量的指针
        );
        if success.0 != 0 {
            return Err(windows_core::Error::from_hresult(success.into()));
        }
        resources.push(HLOCAL(p_acl as *mut c_void)); // 记录需释放的 ACL
    }

    // 分配并初始化安全描述符
    let p_sd: *mut PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    resources.push(HLOCAL(p_sd as *mut c_void));

    unsafe {
        InitializeSecurityDescriptor(
            *p_sd,
            SECURITY_DESCRIPTOR_REVISION, // 要分配给安全描述符的修订级别
        )?;

        // 将 ACL 附加到安全描述符
        SetSecurityDescriptorDacl(
            *p_sd, // 指向函数将 DACL 添加到的 SECURITY_DESCRIPTOR 结构的指针
            true, // 安全描述符中存在 DACL 的标志
            Some(p_acl), // 指向 ACL 结构的指针，该结构指定安全描述符的 DACL
            false, // DACL 已由用户显式指定
        )?;
    }

    // 初始化 SECURITY_ATTRIBUTES
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32, // 结构体的大小
        lpSecurityDescriptor: p_sd as *mut c_void,
        bInheritHandle: BOOL::from(false), // 在创建新进程时不继承返回的句柄
    };
    
    Ok((sa, resources))
}

// 释放安全描述符相关资源（避免内存泄漏）
fn free_sa_resources(resources: Vec<HLOCAL>) {
    for h in resources {
        if !h.0.is_null() {
            unsafe {
                LocalFree(Some(h));
            }
        }
    }
}
