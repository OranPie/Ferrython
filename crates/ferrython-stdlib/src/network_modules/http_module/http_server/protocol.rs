use super::*;

pub(super) fn guess_content_type(path: &str) -> &'static str {
    if let Some(ext) = path.rsplit('.').next() {
        match ext.to_ascii_lowercase().as_str() {
            "html" | "htm" => "text/html; charset=utf-8",
            "css" => "text/css",
            "js" => "application/javascript",
            "json" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "svg" => "image/svg+xml",
            "ico" => "image/x-icon",
            "txt" => "text/plain; charset=utf-8",
            "xml" => "application/xml",
            "pdf" => "application/pdf",
            "wasm" => "application/wasm",
            "zip" => "application/zip",
            "gz" | "tgz" => "application/gzip",
            "tar" => "application/x-tar",
            "mp3" => "audio/mpeg",
            "mp4" => "video/mp4",
            "webp" => "image/webp",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            _ => "application/octet-stream",
        }
    } else {
        "application/octet-stream"
    }
}

pub(super) fn http_status_reason(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        _ => "Unknown",
    }
}

pub(super) struct HttpRequest {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) version: String,
    pub(super) headers: IndexMap<String, String>,
    pub(super) body: Vec<u8>,
}

pub(super) fn parse_http_request(reader: &mut BufReader<&mut TcpStream>) -> Option<HttpRequest> {
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).ok()? == 0 {
        return None;
    }
    let request_line = request_line.trim_end().to_string();
    let parts: Vec<&str> = request_line.splitn(3, ' ').collect();
    if parts.len() < 2 {
        return None;
    }
    let method = parts[0].to_string();
    let path = parts[1].to_string();
    let version = if parts.len() > 2 {
        parts[2].to_string()
    } else {
        "HTTP/1.0".to_string()
    };

    let mut headers = IndexMap::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(idx) = trimmed.find(':') {
            let key = trimmed[..idx].trim().to_string();
            let val = trimmed[idx + 1..].trim().to_string();
            headers.insert(key, val);
        }
    }

    let body = if let Some(cl) = headers
        .get("Content-Length")
        .or_else(|| headers.get("content-length"))
    {
        if let Ok(len) = cl.parse::<usize>() {
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).ok()?;
            buf
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Some(HttpRequest {
        method,
        path,
        version,
        headers,
        body,
    })
}

pub(super) fn write_error_response(stream: &mut TcpStream, code: u16, message: &str) {
    let reason = http_status_reason(code);
    let body = format!(
        "<html><head><title>Error {code}</title></head>\
         <body><h1>Error {code}: {reason}</h1><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
