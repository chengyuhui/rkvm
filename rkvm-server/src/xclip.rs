use anyhow::Result;

use crate::ClipboardType;

async fn xclip_get(target: &str) -> Result<Vec<u8>> {
    let output = tokio::process::Command::new("xclip")
        .arg("-selection")
        .arg("clipboard")
        .arg("-t")
        .arg(target)
        .arg("-o")
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!("xclip failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    Ok(output.stdout)
}

pub async fn get_xclip_timestamp() -> Result<Option<u64>> {
    let targets_str = String::from_utf8(xclip_get("TARGETS").await?)?;
    let targets = targets_str.split('\n').collect::<Vec<_>>();

    if targets.contains(&"TIMESTAMP") {
        let timestamp = xclip_get("TIMESTAMP").await?;
        let timestamp = String::from_utf8_lossy(&timestamp).to_string();
        let timestamp = timestamp.trim().parse::<u64>()?;
        return Ok(Some(timestamp));
    }

    Ok(None)
}

pub async fn get_xclip_clipboard() -> Result<Option<ClipboardType>> {
    let targets_str = String::from_utf8(xclip_get("TARGETS").await?)?;
    let targets = targets_str.split('\n').collect::<Vec<_>>();

    if targets.contains(&"image/png") {
        let image = xclip_get("image/png").await?;
        return Ok(Some(ClipboardType::PngImage(image)));
    }

    let html_text = if targets.contains(&"text/html") {
        let html = xclip_get("text/html").await?;
        Some(String::from_utf8_lossy(&html).to_string())
    } else {
        None
    };

    let text_types = [
        "UTF8_STRING",
        "text/plain;charset=utf-8",
        "text/plain;charset=UTF-8",
        "TEXT",
    ];

    for text_type in &text_types {
        if targets.contains(text_type) {
            let text = xclip_get(text_type).await?;
            let decoded = String::from_utf8_lossy(&text).to_string();

            if let Some(html_text) = html_text {
                return Ok(Some(ClipboardType::HtmlText {
                    html: html_text,
                    plain: decoded,
                }));
            } else {
                return Ok(Some(ClipboardType::Utf8Text(decoded)));
            }
        }
    }

    Ok(None)
}

