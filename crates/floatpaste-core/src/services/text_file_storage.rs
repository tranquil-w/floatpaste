//! 文本上屏生成的附件缓存；独立 UUID 目录避免同名覆盖，发布后保留七天。
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use uuid::Uuid;

use crate::domain::error::AppError;

const RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub fn normalize_file_name(name: &str) -> Result<String, AppError> {
    let invalid =
        || AppError::Message("请输入有效文件名，不能包含路径、控制字符或 Windows 保留名称".into());
    if name.is_empty()
        || name.trim().is_empty()
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return Err(invalid());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|prefix| {
        stem.strip_prefix(prefix).is_some_and(|n| {
            matches!(
                n,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
    }) {
        return Err(invalid());
    }
    let result = if Path::new(name).extension().is_none() {
        format!("{name}.txt")
    } else {
        name.to_string()
    };
    if result.encode_utf16().count() > 255 {
        return Err(AppError::Message(
            "文件名过长，请缩短到 255 个 UTF-16 码元以内".into(),
        ));
    }
    Ok(result)
}

#[derive(Clone)]
pub struct TextFileStorage {
    root: PathBuf,
    gate: Arc<Mutex<()>>,
}

impl TextFileStorage {
    pub(crate) fn new(base: &Path) -> Self {
        Self {
            root: base.join("text-files"),
            gate: Arc::default(),
        }
    }

    pub fn create(&self, name: &str, text: &str) -> Result<PathBuf, AppError> {
        let name = normalize_file_name(name)?;
        super::input_limits::validate_text(text)?;
        let _gate = self.gate.lock()?;
        fs::create_dir_all(&self.root)?;
        real_directory(&self.root)?;
        let dir = self.root.join(Uuid::new_v4().to_string());
        fs::create_dir(&dir)?;
        let path = dir.join(name);
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(text.as_bytes())?;
            file.flush()?;
            Ok::<_, AppError>(())
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_dir(&dir);
            return Err(error);
        }
        Ok(path)
    }

    /// 仅撤销本次未能发布的附件，不触及用户选定的路径。
    pub fn discard(&self, path: &Path) -> Result<(), AppError> {
        let _gate = self.gate.lock()?;
        let dir = path
            .parent()
            .filter(|dir| dir.parent() == Some(self.root.as_path()))
            .ok_or_else(|| AppError::Message("拒绝清理非应用附件路径".into()))?;
        self.remove_directory(dir)
    }

    /// 清除历史时清空；日常清理只回收超过七天的附件。失败项留待下轮重试。
    pub fn cleanup(&self, all: bool) -> Result<usize, AppError> {
        self.cleanup_at(all, SystemTime::now())
    }

    fn cleanup_at(&self, all: bool, now: SystemTime) -> Result<usize, AppError> {
        let _gate = self.gate.lock()?;
        if !self.root.try_exists()? {
            return Ok(0);
        }
        real_directory(&self.root)?;
        let mut pending = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if Uuid::parse_str(&entry.file_name().to_string_lossy()).is_err() {
                continue;
            }
            let result = (|| {
                let metadata = real_directory(&entry.path())?;
                if all || now.duration_since(metadata.modified()?).unwrap_or_default() >= RETENTION
                {
                    self.remove_directory(&entry.path())?;
                }
                Ok::<_, AppError>(())
            })();
            if let Err(error) = result {
                pending += 1;
                tracing::warn!(error = %error, "应用文本附件清理失败，将于下轮重试");
            }
        }
        Ok(pending)
    }

    fn remove_directory(&self, dir: &Path) -> Result<(), AppError> {
        real_directory(&self.root)?;
        real_directory(dir)?;
        if dir.parent() != Some(self.root.as_path())
            || dir
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .is_none()
        {
            return Err(AppError::Message("拒绝清理非应用附件目录".into()));
        }
        // 不递归、不跟随重解析点；目录内只允许本功能生成的普通文件。
        let entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
        for entry in &entries {
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.is_file() || metadata.file_attributes() & 0x400 != 0 {
                return Err(AppError::Message("附件目录含非普通文件，拒绝清理".into()));
            }
        }
        for entry in entries {
            fs::remove_file(entry.path())?;
        }
        fs::remove_dir(dir)?;
        Ok(())
    }
}

fn real_directory(path: &Path) -> Result<fs::Metadata, AppError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_attributes() & 0x400 != 0 {
        return Err(AppError::Message(
            "附件目录不是普通目录，拒绝读写或清理".into(),
        ));
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_preserve_extensions_and_reject_windows_paths_and_devices() {
        assert_eq!(normalize_file_name("文本").unwrap(), "文本.txt");
        for name in ["文本.txt", "笔记.md", "示例.py", "配置.json", "a.b.csv"] {
            assert_eq!(normalize_file_name(name).unwrap(), name);
        }
        for name in [
            "",
            " ",
            ".",
            "..",
            "a.",
            "a ",
            "../a",
            "C:\\a.txt",
            "a:b",
            "a\n.txt",
            "CON.txt",
            "lpt1.py",
            "COM¹",
            "AUX",
            "NUL.json",
        ] {
            assert!(normalize_file_name(name).is_err(), "{name:?}");
        }
        assert!(normalize_file_name(&"中".repeat(256)).is_err());
        assert_eq!(
            normalize_file_name(&"中".repeat(251))
                .unwrap()
                .encode_utf16()
                .count(),
            255
        );
        assert!(normalize_file_name(&"😀".repeat(126)).is_err());
    }

    #[test]
    fn generated_files_preserve_bytes_isolate_names_and_expire_without_touching_sources() {
        let base = std::env::temp_dir().join(format!("floatpaste-text-files-{}", Uuid::new_v4()));
        fs::create_dir(&base).unwrap();
        let storage = TextFileStorage::new(&base);
        let text = "中文 😀\r\n    缩进  \n```py\nprint(1)\n```";
        let first = storage.create("代码.py", text).unwrap();
        let second = storage.create("代码.py", "第二次").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), text.as_bytes());
        let source = base.join("用户源文件.txt");
        fs::write(&source, "保留").unwrap();
        assert!(storage.discard(&source).is_err());
        storage.cleanup(false).unwrap();
        assert!(first.exists());
        storage
            .cleanup_at(
                false,
                SystemTime::now() + RETENTION + Duration::from_secs(1),
            )
            .unwrap();
        assert!(!first.exists());
        assert!(!second.exists());
        assert_eq!(fs::read_to_string(source).unwrap(), "保留");
        let third = storage.create("文本", text).unwrap();
        assert_eq!(third.file_name().unwrap(), "文本.txt");
        storage.cleanup(true).unwrap();
        assert!(!third.exists());
        fs::remove_dir_all(base).unwrap();
    }
}
