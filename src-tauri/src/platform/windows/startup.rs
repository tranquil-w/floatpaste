use windows::{
    core::{HRESULT, PCWSTR},
    Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    },
};

use crate::domain::error::AppError;

use super::wide_string::to_wide;

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const ERROR_FILE_NOT_FOUND_HRESULT: HRESULT = HRESULT(0x80070002u32 as i32);

pub fn sync_run_entry(entry_name: &str, value: Option<&str>) -> Result<(), AppError> {
    let mut key = HKEY::default();
    let path = to_wide(RUN_KEY_PATH);
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR::from_raw(path.as_ptr()),
            Some(0),
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()?;
    }

    let result = match value {
        Some(command) => set_string_value(key, entry_name, command),
        None => delete_value(key, entry_name),
    };

    let _ = unsafe { RegCloseKey(key) };
    result
}

fn set_string_value(key: HKEY, name: &str, value: &str) -> Result<(), AppError> {
    let name = to_wide(name);
    let value = to_wide(value);
    let bytes = unsafe {
        std::slice::from_raw_parts(
            value.as_ptr() as *const u8,
            value.len() * std::mem::size_of::<u16>(),
        )
    };

    unsafe {
        RegSetValueExW(
            key,
            PCWSTR::from_raw(name.as_ptr()),
            Some(0),
            REG_SZ,
            Some(bytes),
        )
        .ok()
        .map_err(AppError::from)
    }
}

fn delete_value(key: HKEY, name: &str) -> Result<(), AppError> {
    let name = to_wide(name);
    match unsafe { RegDeleteValueW(key, PCWSTR::from_raw(name.as_ptr())) }.ok() {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_FILE_NOT_FOUND_HRESULT => Ok(()),
        Err(error) => Err(AppError::from(error)),
    }
}

#[cfg(test)]
mod tests {
    use windows::core::HRESULT;

    use super::ERROR_FILE_NOT_FOUND_HRESULT;

    #[test]
    fn file_not_found_hresult_matches_missing_registry_value() {
        assert_eq!(ERROR_FILE_NOT_FOUND_HRESULT, HRESULT(0x80070002u32 as i32));
    }
}
