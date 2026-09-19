//! 自启任务：Windows 任务计划程序（COM ITaskService）。
//!
//! 「开机自启」与「以管理员权限启动」共用的唯一载体（对齐 PowerToys 的
//! 行为逻辑）：任务存在 = 开机自启开启；任务 RunLevel = 是否提权。任务
//! ACL 仅授予 SYSTEM/Administrators/任务所属用户完全控制——非提权的本
//! 进程也能查询/删除/重建自己的任务，只有注册 HIGHEST 任务需要提权
//! （由壳层经 UAC 重入自身完成）。

use windows::core::{BSTR, Interface};
use windows::Win32::Foundation::{CloseHandle, HANDLE, VARIANT_BOOL};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ILogonTrigger, ITaskDefinition, ITaskService, TASK_ACTION_EXEC,
    TASK_CREATE_OR_UPDATE, TASK_INSTANCES_IGNORE_NEW, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_RUNLEVEL_HIGHEST, TASK_RUNLEVEL_LUA, TASK_RUNLEVEL_TYPE, TASK_TRIGGER_LOGON, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use crate::domain::error::AppError;

/// 登录延迟：等 Explorer 就绪再启动（托盘/剪贴板监听依赖桌面就绪）
const LOGON_TRIGGER_DELAY: &str = "PT03S";
/// 无限执行时限（默认 72h 限制会杀掉常驻进程）
const NO_TIME_LIMIT: &str = "PT0S";

/// 任务当前状态快照（自启同步的真值来源）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub present: bool,
    /// RunLevel == HIGHEST
    pub highest: bool,
    /// ExecAction 参数（随「开机时静默启动」设置变化）
    pub arguments: String,
}

impl TaskSnapshot {
    fn absent() -> Self {
        Self {
            present: false,
            highest: false,
            arguments: String::new(),
        }
    }
}

/// 查询本用户自启任务的状态
pub fn query() -> Result<TaskSnapshot, AppError> {
    unsafe {
        let service = connect()?;
        let folder = service.GetFolder(&BSTR::from("\\"))?;
        let task = match folder.GetTask(&BSTR::from(task_name())) {
            Ok(task) => task,
            Err(error) if is_task_not_found(&error) => return Ok(TaskSnapshot::absent()),
            Err(error) => return Err(error.into()),
        };
        let definition = task.Definition()?;
        let mut run_level = TASK_RUNLEVEL_TYPE::default();
        definition.Principal()?.RunLevel(&mut run_level)?;
        let highest = run_level == TASK_RUNLEVEL_HIGHEST;
        Ok(TaskSnapshot {
            present: true,
            highest,
            arguments: first_exec_arguments(&definition)?,
        })
    }
}

/// 注册（或覆盖）自启任务：登录当前用户时启动，RunLevel 按提权开关
pub fn install(executable: &str, arguments: &str, highest: bool) -> Result<(), AppError> {
    let user_id = current_user_id();
    unsafe {
        let service = connect()?;
        let task: ITaskDefinition = service.NewTask(0)?;

        let principal = task.Principal()?;
        principal.SetRunLevel(if highest {
            TASK_RUNLEVEL_HIGHEST
        } else {
            TASK_RUNLEVEL_LUA
        })?;
        principal.SetLogonType(TASK_LOGON_INTERACTIVE_TOKEN)?;
        principal.SetUserId(&BSTR::from(&user_id))?;

        let settings = task.Settings()?;
        // 常驻进程：不受执行时限与电池策略约束；已在运行则忽略新实例
        settings.SetExecutionTimeLimit(&BSTR::from(NO_TIME_LIMIT))?;
        settings.SetDisallowStartIfOnBatteries(VARIANT_BOOL(0))?;
        settings.SetStopIfGoingOnBatteries(VARIANT_BOOL(0))?;
        settings.SetMultipleInstances(TASK_INSTANCES_IGNORE_NEW)?;

        let trigger = task.Triggers()?.Create(TASK_TRIGGER_LOGON)?;
        let logon = trigger.cast::<ILogonTrigger>()?;
        logon.SetUserId(&BSTR::from(&user_id))?;
        logon.SetDelay(&BSTR::from(LOGON_TRIGGER_DELAY))?;

        let exec = task.Actions()?.Create(TASK_ACTION_EXEC)?;
        let exec = exec.cast::<IExecAction>()?;
        exec.SetPath(&BSTR::from(executable))?;
        exec.SetArguments(&BSTR::from(arguments))?;

        let folder = service.GetFolder(&BSTR::from("\\"))?;
        let registered = folder.RegisterTaskDefinition(
            &BSTR::from(task_name()),
            &task,
            TASK_CREATE_OR_UPDATE.0,
            &VARIANT::default(),
            &VARIANT::default(),
            TASK_LOGON_INTERACTIVE_TOKEN,
            &VARIANT::default(),
        )?;
        // 收紧 ACL：SYSTEM/Administrators/本用户之外不可写——既防其他本地
        // 用户篡改任务动作提权，也让非提权的本进程后续能删除/重建任务
        registered.SetSecurityDescriptor(&BSTR::from(task_sddl()?), 0)?;
    }
    Ok(())
}

/// 删除自启任务；任务不存在视为已卸载（普通权限即可完成）
pub fn uninstall() -> Result<(), AppError> {
    unsafe {
        let service = connect()?;
        let folder = service.GetFolder(&BSTR::from("\\"))?;
        if let Err(error) = folder.DeleteTask(&BSTR::from(task_name()), 0) {
            if !is_task_not_found(&error) {
                return Err(error.into());
            }
        }
    }
    Ok(())
}

/// 任务名带用户名：多用户各自注册互不覆盖
fn task_name() -> String {
    format!("FloatPaste for {}", std::env::var("USERNAME").unwrap_or_default())
}

fn current_user_id() -> String {
    format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    )
}

/// 当前用户 SID 的 SDDL DACL：SYSTEM 与 Administrators 永远完全控制，
/// 追加任务所属用户；绝不授予 Everyone 写权限（本地提权风险）
fn task_sddl() -> Result<String, AppError> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let mut buffer = [0u8; std::mem::size_of::<TOKEN_USER>() + SECURITY_MAX_SID_SIZE as usize];
        let mut returned: u32 = 0;
        let result = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut _),
            buffer.len() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        result?;

        let user = buffer.as_ptr() as *const TOKEN_USER;
        let mut sid_string = windows::core::PWSTR::null();
        ConvertSidToStringSidW((*user).User.Sid, &mut sid_string)?;
        let sid = sid_string
            .to_string()
            .map_err(|error| AppError::Message(error.to_string()))?;
        let _ = LocalFree(Some(HLOCAL(sid_string.as_ptr().cast())));
        Ok(format!("D:(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;{sid})"))
    }
}

/// 首个 Exec 动作的参数（COM 集合下标从 1 起）
unsafe fn first_exec_arguments(definition: &ITaskDefinition) -> Result<String, AppError> {
    unsafe {
        let actions = definition.Actions()?;
        let mut count = 0i32;
        actions.Count(&mut count)?;
        for index in 1..=count {
            if let Ok(exec) = actions.get_Item(index)?.cast::<IExecAction>() {
                let mut arguments = BSTR::new();
                exec.Arguments(&mut arguments)?;
                return Ok(arguments.to_string());
            }
        }
    }
    Ok(String::new())
}

/// 任务不存在的两种 HRESULT：SCHED_E_TASK_NOT_FOUND（0x80041308）与
/// ERROR_FILE_NOT_FOUND（0x80070002，任务计划底层按任务文件缺失报错）
fn is_task_not_found(error: &windows::core::Error) -> bool {
    matches!(error.code().0 as u32 & 0xFFFF, 0x1308 | 0x0002)
}

/// COM 初始化 + 连接本机任务计划服务（每次操作独立连接，简单可靠）
unsafe fn connect() -> Result<ITaskService, AppError> {
    unsafe {
        // 已初始化为其他模型（S_FALSE / RPC_E_CHANGED_MODE）均不影响使用
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let service: ITaskService =
            // Schedule.Service coclass（ITaskService v2.0）；CLSID_CTaskScheduler
            // 是老版任务计划 1.0，查 ITaskService 会报 E_NOINTERFACE
            CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)?;
        service.Connect(
            &VARIANT::default(),
            &VARIANT::default(),
            &VARIANT::default(),
            &VARIANT::default(),
        )?;
        Ok(service)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_task_not_found, query};

    #[test]
    fn sched_e_task_not_found_is_recognized() {
        // SCHED_E_TASK_NOT_FOUND = 0x80041308
        let error = windows::core::Error::from_hresult(windows::core::HRESULT(0x80041308u32 as i32));
        assert!(is_task_not_found(&error));
        assert!(!is_task_not_found(&windows::core::Error::from_hresult(
            windows::core::HRESULT(0x80070005u32 as i32)
        )));
    }

    #[test]
    fn task_query_connects_and_reports_snapshot() {
        // 只读查询（COM 连接 + GetTask），不需要提权；注册与否都返回 Ok，
        // 此处验证 COM 绑定与连接在真实系统上可用
        let _ = query().expect("任务计划查询应可用");
    }
}
