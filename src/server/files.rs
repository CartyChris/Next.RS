//! Static files: hashed build assets and `public/`.

use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};

pub fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("txt" | "md" | "glsl" | "vert" | "frag" | "wgsl") => "text/plain; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("bmp") => "image/bmp",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        Some("m4a") => "audio/mp4",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("wasm") => "application/wasm",
        Some("pdf") => "application/pdf",
        Some("glb") => "model/gltf-binary",
        Some("gltf") => "model/gltf+json",
        Some("ktx2") => "image/ktx2",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// Resolve `rel` under `dir` without escaping it.
fn safe_join(dir: &Path, rel: &str) -> Option<PathBuf> {
    let parts = crate::routes::split_path(rel)?;
    if parts.is_empty() || parts.iter().any(|p| p.starts_with('.')) {
        return None;
    }
    let mut p = dir.to_path_buf();
    p.extend(parts);
    Some(p)
}

fn accepts(headers: &HeaderMap, encoding: &str) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|e| e.trim().split(';').next() == Some(encoding) && !e.contains("q=0"))
}

async fn file_body(path: &Path) -> std::io::Result<(Body, u64)> {
    let file = tokio::fs::File::open(path).await?;
    let len = file.metadata().await?.len();
    if len <= 256 * 1024 {
        let mut buf = Vec::with_capacity(len as usize);
        tokio::io::AsyncReadExt::read_to_end(&mut tokio::io::BufReader::new(file), &mut buf).await?;
        return Ok((Body::from(buf), len));
    }
    let stream = futures_util::stream::unfold(file, |mut f| async move {
        let mut buf = vec![0u8; 64 * 1024];
        match tokio::io::AsyncReadExt::read(&mut f, &mut buf).await {
            Ok(0) => None,
            Ok(n) => {
                buf.truncate(n);
                Some((Ok::<_, std::io::Error>(bytes::Bytes::from(buf)), f))
            }
            Err(e) => Some((Err(e), f)),
        }
    });
    Ok((Body::from_stream(stream), len))
}

/// Serve `rel` from `dir`, preferring precompressed `.br` / `.gz` siblings.
pub async fn serve(dir: &Path, rel: &str, headers: &HeaderMap, immutable: bool) -> Response {
    match serve_file(dir, rel, headers, immutable).await {
        Some(r) => r,
        None => Response::builder().status(StatusCode::NOT_FOUND).body(Body::from("not found")).unwrap(),
    }
}

pub async fn serve_public(dir: &Path, path: &str, headers: &HeaderMap) -> Option<Response> {
    serve_file(dir, path, headers, false).await
}

async fn serve_file(dir: &Path, rel: &str, headers: &HeaderMap, immutable: bool) -> Option<Response> {
    let path = safe_join(dir, rel)?;
    let meta = tokio::fs::metadata(&path).await.ok().filter(|m| m.is_file())?;
    let etag = format!(
        "\"{:x}-{:x}\"",
        meta.len(),
        meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis())
    );
    let cache = if immutable { "public, max-age=31536000, immutable" } else { "public, max-age=0, must-revalidate" };
    if headers.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) == Some(etag.as_str()) {
        return Some(
            Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag)
                .header(header::CACHE_CONTROL, cache)
                .body(Body::empty())
                .unwrap(),
        );
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let mut chosen = (path.clone(), None);
    for (enc, ext) in [("br", "br"), ("gzip", "gz")] {
        let candidate = path.with_file_name(format!("{name}.{ext}"));
        if accepts(headers, enc) && tokio::fs::metadata(&candidate).await.is_ok() {
            chosen = (candidate, Some(enc));
            break;
        }
    }
    let (body, len) = file_body(&chosen.0).await.ok()?;
    let mut r = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime(&path))
        .header(header::CONTENT_LENGTH, len)
        .header(header::CACHE_CONTROL, cache)
        .header(header::ETAG, etag)
        .header(header::VARY, "accept-encoding")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(body)
        .unwrap();
    if let Some(enc) = chosen.1 {
        r.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(enc));
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_join_stays_inside() {
        let d = Path::new("/srv");
        assert_eq!(safe_join(d, "a/b.js"), Some(PathBuf::from("/srv/a/b.js")));
        assert_eq!(safe_join(d, "../etc/passwd"), None);
        assert_eq!(safe_join(d, "%2e%2e/x"), None);
        assert_eq!(safe_join(d, ".env"), None);
        assert_eq!(safe_join(d, ""), None);
    }

    #[test]
    fn accept_encoding_parsing() {
        let mut h = HeaderMap::new();
        h.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip, deflate, br;q=1.0"));
        assert!(accepts(&h, "br") && accepts(&h, "gzip") && !accepts(&h, "zstd"));
        h.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("br;q=0, gzip"));
        assert!(!accepts(&h, "br"));
    }
}
