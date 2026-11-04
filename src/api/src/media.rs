//! 圖片和音訊：存檔、判斷格式、送給模型前縮小。

use std::{io::Cursor, path::Path};

use anyhow::{Context, Result};
use base64::Engine;
use image::{GenericImageView, ImageFormat};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{db::now, models::Tx};

/// 送給模型的圖片，長邊超過這個像素就縮小。Claude 建議的上限是 1568（再大也會被縮小，只是浪費頻寬）。
pub const LLM_IMAGE_MAX_EDGE: u32 = 1568;
/// 送給模型的圖片檔案大小上限。Claude 單張圖片的上限是 5 MB（base64 前要再小一點）。
pub const LLM_IMAGE_MAX_BYTES: usize = 3_750_000;
const JPEG_QUALITY: u8 = 85;

/// 認得的媒體格式：副檔名、MIME、種類。圖片另外用檔頭確認。不收 SVG（可以藏 JavaScript）。
const FORMATS: &[(&str, &str, &str)] = &[
    ("png", "image/png", "image"),
    ("jpg", "image/jpeg", "image"),
    ("jpeg", "image/jpeg", "image"),
    ("gif", "image/gif", "image"),
    ("webp", "image/webp", "image"),
    ("bmp", "image/bmp", "image"),
    ("mp3", "audio/mpeg", "audio"),
    ("wav", "audio/wav", "audio"),
    ("ogg", "audio/ogg", "audio"),
    ("oga", "audio/ogg", "audio"),
    ("m4a", "audio/mp4", "audio"),
    ("flac", "audio/flac", "audio"),
    ("webm", "audio/webm", "audio"),
];

/// Claude 和大多數 OpenAI 相容服務直接收的圖片格式，其他格式要先轉成 PNG。
const LLM_IMAGE_MIMES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub kind: &'static str,
    pub mime: &'static str,
    pub ext: &'static str,
}

/// 依檔名判斷是不是認得的媒體檔（不讀內容）。
pub fn is_media_name(name: &str) -> bool {
    extension(name).is_some_and(|e| FORMATS.iter().any(|(x, ..)| *x == e))
}

fn extension(name: &str) -> Option<String> {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
}

/// 判斷格式：圖片看檔頭，音訊看副檔名或 MIME。
pub fn detect(bytes: &[u8], name: &str, mime_hint: Option<&str>) -> Option<Detected> {
    if let Ok(fmt) = image::guess_format(bytes) {
        let mime = match fmt {
            ImageFormat::Png => "image/png",
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Gif => "image/gif",
            ImageFormat::WebP => "image/webp",
            ImageFormat::Bmp => "image/bmp",
            _ => return None,
        };
        let ext = FORMATS.iter().find(|(_, m, _)| *m == mime).map(|(e, ..)| *e)?;
        return Some(Detected {
            kind: "image",
            mime,
            ext,
        });
    }
    let by_ext = extension(name).and_then(|e| FORMATS.iter().find(|(x, _, k)| *x == e && *k == "audio"));
    let by_mime = mime_hint.and_then(|m| FORMATS.iter().find(|(_, x, k)| *x == m && *k == "audio"));
    by_ext.or(by_mime).map(|(ext, mime, kind)| Detected { kind, mime, ext })
}

/// 解析 `data:image/png;base64,....`。
pub fn decode_data_url(url: &str) -> Option<(Option<String>, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    let mime = header.strip_suffix(";base64")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).ok()?;
    Some(((!mime.is_empty()).then(|| mime.to_string()), bytes))
}

/// 存一個媒體檔：同樣內容只存一份。回傳 (media id, sha256)。
pub async fn store(
    tx: &mut Tx<'_>,
    media_dir: &Path,
    bytes: &[u8],
    name: &str,
    mime_hint: Option<&str>,
) -> Result<(i64, String)> {
    let detected = detect(bytes, name, mime_hint).with_context(|| {
        format!("「{name}」不是支援的圖片或音訊格式（支援 png、jpg、gif、webp、bmp、mp3、wav、ogg、m4a、flac、webm）")
    })?;
    let sha = format!("{:x}", Sha256::digest(bytes));
    if let Some(id) = sqlx::query_scalar("SELECT id FROM media WHERE sha256 = ?")
        .bind(&sha)
        .fetch_optional(&mut **tx)
        .await?
    {
        return Ok((id, sha));
    }
    let rel = format!("{}/{}.{}", &sha[..2], sha, detected.ext);
    let path = media_dir.join(&rel);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // 先寫暫存檔再改名，避免寫到一半的檔案被讀到
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
    }
    let id = sqlx::query_scalar(
        "INSERT INTO media (sha256, kind, mime, size, path, original_name, created_at) VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(&sha)
    .bind(detected.kind)
    .bind(detected.mime)
    .bind(bytes.len() as i64)
    .bind(&rel)
    .bind(name)
    .bind(now())
    .fetch_one(&mut **tx)
    .await?;
    Ok((id, sha))
}

/// 刪掉沒有任何資料用到的媒體檔（刪專案後呼叫）。
pub async fn remove_orphans(db: &SqlitePool, media_dir: &Path) -> Result<usize> {
    let orphans: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, path FROM media WHERE id NOT IN (SELECT media_id FROM items WHERE media_id IS NOT NULL)",
    )
    .fetch_all(db)
    .await?;
    for (id, rel) in &orphans {
        sqlx::query("DELETE FROM media WHERE id = ?")
            .bind(id)
            .execute(db)
            .await?;
        let _ = tokio::fs::remove_file(media_dir.join(rel)).await;
    }
    Ok(orphans.len())
}

/// 送給模型的圖片（base64）。
#[derive(Clone, Debug)]
pub struct LlmImage {
    pub mime: String,
    pub base64: String,
}

impl LlmImage {
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.base64)
    }
}

/// 把圖片整理成模型收得了的樣子：太大就縮小，格式不支援就轉 PNG，其他情況原封不動。
pub fn prepare_for_llm(bytes: &[u8], mime: &str) -> Result<LlmImage> {
    let encode = |b: &[u8], m: &str| LlmImage {
        mime: m.to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(b),
    };
    let img = image::load_from_memory(bytes).context("圖片無法解碼")?;
    let (w, h) = img.dimensions();
    let supported = LLM_IMAGE_MIMES.contains(&mime);
    if supported && w.max(h) <= LLM_IMAGE_MAX_EDGE && bytes.len() <= LLM_IMAGE_MAX_BYTES {
        return Ok(encode(bytes, mime));
    }
    let img = if w.max(h) > LLM_IMAGE_MAX_EDGE {
        img.resize(
            LLM_IMAGE_MAX_EDGE,
            LLM_IMAGE_MAX_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let mut out = Cursor::new(Vec::new());
    if img.color().has_alpha() {
        img.write_to(&mut out, ImageFormat::Png)?;
        Ok(encode(out.get_ref(), "image/png"))
    } else {
        let rgb = img.to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode_image(&rgb)?;
        Ok(encode(out.get_ref(), "image/jpeg"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([20, 160, 150]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn detects_images_by_content_and_audio_by_name() {
        let d = detect(&png(2, 2), "wrong-name.txt", None).unwrap();
        assert_eq!((d.kind, d.mime, d.ext), ("image", "image/png", "png"));
        let a = detect(b"ID3....", "clip.MP3", None).unwrap();
        assert_eq!((a.kind, a.mime), ("audio", "audio/mpeg"));
        assert!(detect(b"<svg></svg>", "x.svg", Some("image/svg+xml")).is_none());
        assert!(is_media_name("a/b/C.JPG"));
        assert!(!is_media_name("notes.txt"));
    }

    #[test]
    fn data_urls() {
        let (mime, bytes) = decode_data_url("data:image/png;base64,aGk=").unwrap();
        assert_eq!(mime.as_deref(), Some("image/png"));
        assert_eq!(bytes, b"hi");
        assert!(decode_data_url("https://example.com/a.png").is_none());
    }

    #[test]
    fn small_images_pass_through_and_big_ones_shrink() {
        let small = png(10, 10);
        let out = prepare_for_llm(&small, "image/png").unwrap();
        assert_eq!(out.mime, "image/png");
        assert_eq!(
            base64::engine::general_purpose::STANDARD.decode(&out.base64).unwrap(),
            small
        );

        let big = prepare_for_llm(&png(3000, 1000), "image/png").unwrap();
        assert_eq!(big.mime, "image/jpeg");
        let decoded =
            image::load_from_memory(&base64::engine::general_purpose::STANDARD.decode(&big.base64).unwrap()).unwrap();
        assert_eq!(decoded.dimensions(), (LLM_IMAGE_MAX_EDGE, 523));
        assert!(big.data_url().starts_with("data:image/jpeg;base64,"));
    }
}
