use std::io::Read;

use anyhow::Result;

use crate::ClipboardType;

pub async fn get_wayland_clipboard() -> Result<Option<ClipboardType>> {
    tokio::task::spawn_blocking(|| {
        let targets = wl_clipboard_rs::paste::get_mime_types(
            wl_clipboard_rs::paste::ClipboardType::Regular,
            wl_clipboard_rs::paste::Seat::Unspecified,
        )?;

        if targets.contains("image/png") {
            let (mut pipe, _) = wl_clipboard_rs::paste::get_contents(
                wl_clipboard_rs::paste::ClipboardType::Regular,
                wl_clipboard_rs::paste::Seat::Unspecified,
                wl_clipboard_rs::paste::MimeType::Specific("image/png"),
            )?;

            let mut image = Vec::new();
            pipe.read_to_end(&mut image)?;

            return Ok(Some(ClipboardType::PngImage(image)));
        }

        let html_text = if targets.contains("text/html") {
            let (mut pipe, _) = wl_clipboard_rs::paste::get_contents(
                wl_clipboard_rs::paste::ClipboardType::Regular,
                wl_clipboard_rs::paste::Seat::Unspecified,
                wl_clipboard_rs::paste::MimeType::Specific("text/html"),
            )?;

            let mut html = String::new();
            pipe.read_to_string(&mut html)?;

            Some(html)
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
            if targets.contains(*text_type) {
                let (mut pipe, _) = wl_clipboard_rs::paste::get_contents(
                    wl_clipboard_rs::paste::ClipboardType::Regular,
                    wl_clipboard_rs::paste::Seat::Unspecified,
                    wl_clipboard_rs::paste::MimeType::Specific(text_type),
                )?;

                let mut text = String::new();
                pipe.read_to_string(&mut text)?;

                if let Some(html_text) = html_text {
                    return Ok(Some(ClipboardType::HtmlText {
                        html: html_text,
                        plain: text,
                    }));
                } else {
                    return Ok(Some(ClipboardType::Utf8Text(text)));
                }
            }
        }

        Ok(None)
    })
    .await?
}
