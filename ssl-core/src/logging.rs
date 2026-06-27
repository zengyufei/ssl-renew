use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn reset_log_if_too_large(log_file: impl AsRef<Path>, max_log_size_mb: f64) -> Result<bool> {
    let log_file = log_file.as_ref();
    if max_log_size_mb <= 0.0 || !log_file.exists() {
        return Ok(false);
    }
    let max_bytes = (max_log_size_mb * 1024.0 * 1024.0) as u64;
    let size = fs::metadata(log_file)
        .with_context(|| format!("读取日志文件大小失败：{}", log_file.display()))?
        .len();
    if size <= max_bytes {
        return Ok(false);
    }
    fs::write(log_file, b"")
        .with_context(|| format!("清空日志文件失败：{}", log_file.display()))?;
    Ok(true)
}

pub fn append_log(log_file: impl AsRef<Path>, text: &str, max_log_size_mb: f64) -> Result<()> {
    let log_file = log_file.as_ref();
    if let Some(parent) = log_file.parent() {
        fs::create_dir_all(parent)?;
    }
    reset_log_if_too_large(log_file, max_log_size_mb)?;
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)?;
    writeln!(file, "{text}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_large_log() {
        let path = std::env::temp_dir().join("ssl_core_reset_large_log.log");
        fs::write(&path, vec![b'x'; 2048]).unwrap();
        assert!(reset_log_if_too_large(&path, 0.001).unwrap());
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
        let _ = fs::remove_file(path);
    }
}
