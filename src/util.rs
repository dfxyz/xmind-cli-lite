use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

pub type Result<T> = std::result::Result<T, AppError>;

#[derive(Debug, Clone)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
    pub exit: i32,
}

impl AppError {
    pub fn new(code: &'static str, exit: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new("usage", 2, message)
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::new("io", 1, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new("invalid_file", 1, message)
    }

    pub fn validation(errors: &[String]) -> Self {
        Self::new("validation_failed", 3, errors.join("; "))
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::io(value.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(value: serde_json::Error) -> Self {
        Self::invalid(value.to_string())
    }
}

impl From<zip::result::ZipError> for AppError {
    fn from(value: zip::result::ZipError) -> Self {
        Self::invalid(value.to_string())
    }
}

pub fn read_stdin() -> Result<String> {
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| AppError::io(format!("读取 stdin 失败: {e}")))?;
    Ok(s)
}

pub struct LoadedImage {
    pub name: String,
    pub bytes: Vec<u8>,
    pub width: u32,
}

pub fn load_image(path: &Path, width: Option<u32>) -> Result<LoadedImage> {
    let bytes = std::fs::read(path)
        .map_err(|e| AppError::io(format!("读取图片 {} 失败: {e}", path.display())))?;
    let ext = match image_ext(&bytes) {
        Some(ext) => ext,
        None => {
            return Err(AppError::usage(format!(
                "不支持的图片格式（仅 PNG/JPEG/GIF/WebP）: {}",
                path.display()
            )));
        }
    };
    if let Some(0) = width {
        return Err(AppError::usage("--width 必须大于 0"));
    }
    let size = imagesize::blob_size(&bytes)
        .map_err(|e| AppError::usage(format!("无法读取图片尺寸 {}: {e}", path.display())))?;
    let intrinsic: u32 = size.width.try_into().unwrap_or(u32::MAX);
    let width = width.unwrap_or_else(|| intrinsic.min(320));
    let hash = Sha256::digest(&bytes);
    let name = format!("{}.{ext}", hex(&hash));
    Ok(LoadedImage { name, bytes, width })
}

fn image_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

pub fn link_target(input: &str, base: &Path) -> String {
    if has_scheme(input) || input.starts_with('#') {
        input.to_string()
    } else {
        file_uri(Path::new(&percent_decode(input)), base)
    }
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn has_scheme(s: &str) -> bool {
    let Some(idx) = s.find(':') else {
        return false;
    };
    idx > 1
        && s.as_bytes()[0].is_ascii_alphabetic()
        && s[..idx]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

pub fn file_uri(path: &Path, base: &Path) -> String {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    let abs = normalize(&abs);
    let mut s = abs.to_string_lossy().replace('\\', "/");
    if s.starts_with("//") {
        return format!("file:{}", encode_path(&s));
    }
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    format!("file://{}", encode_path(&s))
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn encode_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn as_obj_mut(v: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !v.is_object() {
        *v = Value::Object(serde_json::Map::new());
    }
    v.as_object_mut().expect("object")
}
