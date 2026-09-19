//! Win+V 接管：读写 Explorer 的 `DisabledHotkeys` 释放系统 Win+V
//! （见 docs/adr-0001）。写值后重启 Explorer 即生效（无需注销/重启系统，
//! 也不要求关闭系统剪贴板历史）；删除值并重启 Explorer 恢复原状。
//!
//! `DisabledHotkeys` 是字母集合（如 "V"、"VS"），接管只增删本应用的
//! `V` 一个字母，其他字母原样保留；删除后为空则整个值删除。

use windows::{
    core::{HRESULT, PCWSTR},
    Win32::Foundation::WIN32_ERROR,
    Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW, RegSetValueExW, HKEY,
        HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_VALUE_TYPE, REG_SZ,
        RRF_RT_REG_SZ,
    },
};

use crate::domain::error::AppError;

use super::wide_string::to_wide;

const ADVANCED_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced";
const VALUE_NAME: &str = "DisabledHotkeys";
/// 本应用接管的字母（Win+V）
const TARGET_LETTER: char = 'V';
const ERROR_FILE_NOT_FOUND_HRESULT: HRESULT = HRESULT(0x80070002u32 as i32);

/// 读取当前 `DisabledHotkeys` 内容；值不存在时视为空串
fn read_disabled_hotkeys() -> Result<String, AppError> {
    let mut key = HKEY::default();
    let path = to_wide(ADVANCED_KEY_PATH);
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(path.as_ptr()),
            Some(0),
            KEY_QUERY_VALUE,
            &mut key,
        )
        .ok()?;
    }

    let name = to_wide(VALUE_NAME);
    let mut buffer = [0u8; 64]; // REG_SZ 字节流；字母集合远小于此
    let mut size = u32::try_from(buffer.len()).unwrap_or(0);
    let mut kind = REG_VALUE_TYPE::default();
    let status = unsafe {
        RegGetValueW(
            key,
            PCWSTR::null(),
            PCWSTR::from_raw(name.as_ptr()),
            RRF_RT_REG_SZ,
            Some(&mut kind),
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    let _ = unsafe { RegCloseKey(key) };
    if status != WIN32_ERROR(0) {
        // 值不存在等读取失败：未配置过任何禁用热键
        return Ok(String::new());
    }

    let len = (size as usize / 2).saturating_sub(1); // 去掉结尾 NUL
    let wide: Vec<u16> = buffer[..len.min(buffer.len())]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Ok(String::from_utf16_lossy(&wide))
}

/// 接管是否已写入系统配置（`DisabledHotkeys` 含 V）
pub fn is_enabled_in_registry() -> Result<bool, AppError> {
    Ok(read_disabled_hotkeys()?
        .to_ascii_uppercase()
        .contains(TARGET_LETTER))
}

/// 写入接管配置：追加 V 字母（保留既有字母），已存在则无操作
pub fn enable_in_registry() -> Result<(), AppError> {
    let current = read_disabled_hotkeys()?;
    let letters = current.to_ascii_uppercase();
    if letters.contains(TARGET_LETTER) {
        return Ok(());
    }
    write_disabled_hotkeys(&format!("{current}{}", TARGET_LETTER.to_ascii_uppercase()))
}

/// 清除接管配置：移除 V 字母；结果为空则整个值删除（一键回退）
pub fn disable_in_registry() -> Result<(), AppError> {
    let current = read_disabled_hotkeys()?;
    let remaining: String = current
        .chars()
        .filter(|char| char.to_ascii_uppercase() != TARGET_LETTER)
        .collect();
    if remaining.is_empty() {
        delete_disabled_hotkeys()
    } else {
        write_disabled_hotkeys(&remaining)
    }
}

fn open_advanced_key_for_write() -> Result<HKEY, AppError> {
    let mut key = HKEY::default();
    let path = to_wide(ADVANCED_KEY_PATH);
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(path.as_ptr()),
            Some(0),
            KEY_SET_VALUE,
            &mut key,
        )
        .ok()?;
    }
    Ok(key)
}

fn write_disabled_hotkeys(value: &str) -> Result<(), AppError> {
    let key = open_advanced_key_for_write()?;
    let name = to_wide(VALUE_NAME);
    let wide = to_wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            wide.as_ptr() as *const u8,
            wide.len() * std::mem::size_of::<u16>(),
        )
    };
    let result = unsafe {
        RegSetValueExW(key, PCWSTR::from_raw(name.as_ptr()), Some(0), REG_SZ, Some(bytes))
            .ok()
            .map_err(AppError::from)
    };
    let _ = unsafe { RegCloseKey(key) };
    result
}

fn delete_disabled_hotkeys() -> Result<(), AppError> {
    let key = open_advanced_key_for_write()?;
    let name = to_wide(VALUE_NAME);
    let result = unsafe { RegDeleteValueW(key, PCWSTR::from_raw(name.as_ptr())) }
        .ok()
        .map_err(AppError::from);
    let _ = unsafe { RegCloseKey(key) };
    // 值本就不存在 = 已是回退后的目标状态
    match result {
        Err(AppError::Windows(error)) if error.code() == ERROR_FILE_NOT_FOUND_HRESULT => Ok(()),
        other => other,
    }
}

/// 重启 Explorer 使 DisabledHotkeys 变更生效（任务栏会短暂消失）。
/// 经 cmd 分离执行：taskkill 后 start 重启 shell，本进程不受影响
pub fn restart_explorer() {
    use std::os::windows::process::CommandExt;

    let _ = std::process::Command::new("cmd")
        .args([
            "/C",
            "taskkill /F /IM explorer.exe & timeout /T 1 /NOBREAK >NUL & start explorer.exe",
        ])
        .creation_flags(0x0000_0008) // DETACHED_PROCESS
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::TARGET_LETTER;

    #[test]
    fn target_letter_is_v() {
        assert_eq!(TARGET_LETTER, 'V');
    }
}
