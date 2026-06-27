use crate::config::NginxConfig;
use anyhow::{Context, Result};
use tokio::process::Command;

pub async fn restart_nginx(config: &NginxConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }
    if config.restart_mode.trim().eq_ignore_ascii_case("reload") {
        let output = Command::new(&config.exe_path)
            .args(["-s", "reload"])
            .current_dir(&config.working_dir)
            .output()
            .await
            .with_context(|| format!("执行 nginx -s reload 失败：{}", config.exe_path))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("nginx -s reload 执行失败：{}", stderr.trim());
        }
        return Ok(());
    }
    let image = if config.kill_image_name.trim().is_empty() {
        "nginx.exe"
    } else {
        config.kill_image_name.trim()
    };
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", image])
        .output()
        .await;
    Command::new(&config.exe_path)
        .current_dir(&config.working_dir)
        .spawn()
        .with_context(|| format!("启动 Nginx 失败：{}", config.exe_path))?;
    Ok(())
}
