//! 查询当前 Windows 会话；不创建额外线程，通知由剪贴板消息窗接收。
use windows::{
    core::{Error, HRESULT, PWSTR},
    Win32::System::RemoteDesktop::{
        WTSActive, WTSFreeMemory, WTSQuerySessionInformationW, WTSSessionInfoEx, WTSINFOEXW,
        WTS_CURRENT_SESSION, WTS_SESSIONSTATE_UNLOCK,
    },
};

pub fn current_session_locked() -> windows::core::Result<bool> {
    let mut buffer = PWSTR::null();
    let mut bytes = 0;
    unsafe {
        WTSQuerySessionInformationW(
            None,
            WTS_CURRENT_SESSION,
            WTSSessionInfoEx,
            &mut buffer,
            &mut bytes,
        )?;
        let result = if buffer.is_null() || (bytes as usize) < std::mem::size_of::<WTSINFOEXW>() {
            Err(Error::from_hresult(HRESULT(0x8007000Du32 as i32)))
        } else {
            let info = &*buffer.0.cast::<WTSINFOEXW>();
            if info.Level != 1 {
                Err(Error::from_hresult(HRESULT(0x8007000Du32 as i32)))
            } else {
                let state = info.Data.WTSInfoExLevel1;
                Ok(state.SessionState != WTSActive
                    || state.SessionFlags as u32 != WTS_SESSIONSTATE_UNLOCK)
            }
        };
        WTSFreeMemory(buffer.0.cast());
        result
    }
}
