use compact_str::CompactString;
use ferrython_core::error::PyException;
use ferrython_core::object::{make_builtin, make_module, PyObject, PyObjectMethods, PyObjectRef};
use indexmap::IndexMap;

// ── mimetypes module ──

fn ext_to_mime(ext: &str) -> Option<&'static str> {
    match ext.to_lowercase().as_str() {
        // Text
        "html" | "htm" => Some("text/html"),
        "xhtml" => Some("application/xhtml+xml"),
        "css" => Some("text/css"),
        "csv" => Some("text/csv"),
        "txt" | "text" | "log" => Some("text/plain"),
        "rtf" => Some("application/rtf"),
        "md" | "markdown" => Some("text/markdown"),
        "rst" => Some("text/x-rst"),
        "ics" => Some("text/calendar"),
        "vcf" => Some("text/vcard"),
        "tsv" => Some("text/tab-separated-values"),
        // Programming
        "js" | "mjs" => Some("application/javascript"),
        "ts" => Some("application/typescript"),
        "json" => Some("application/json"),
        "jsonld" => Some("application/ld+json"),
        "xml" => Some("application/xml"),
        "xsl" | "xslt" => Some("application/xslt+xml"),
        "dtd" => Some("application/xml-dtd"),
        "py" => Some("text/x-python"),
        "rb" => Some("text/x-ruby"),
        "java" => Some("text/x-java-source"),
        "c" | "h" => Some("text/x-c"),
        "cpp" | "cxx" | "cc" | "hpp" => Some("text/x-c++src"),
        "rs" => Some("text/x-rust"),
        "go" => Some("text/x-go"),
        "sh" | "bash" => Some("application/x-sh"),
        "bat" | "cmd" => Some("application/x-msdos-program"),
        "sql" => Some("application/sql"),
        "php" => Some("application/x-httpd-php"),
        "pl" | "pm" => Some("text/x-perl"),
        "lua" => Some("text/x-lua"),
        // Images
        "jpg" | "jpeg" | "jpe" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "svg" | "svgz" => Some("image/svg+xml"),
        "ico" => Some("image/x-icon"),
        "webp" => Some("image/webp"),
        "tiff" | "tif" => Some("image/tiff"),
        "avif" => Some("image/avif"),
        "heic" | "heif" => Some("image/heif"),
        "psd" => Some("image/vnd.adobe.photoshop"),
        // Audio
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "ogg" | "oga" => Some("audio/ogg"),
        "flac" => Some("audio/flac"),
        "aac" => Some("audio/aac"),
        "m4a" => Some("audio/mp4"),
        "wma" => Some("audio/x-ms-wma"),
        "mid" | "midi" => Some("audio/midi"),
        "opus" => Some("audio/opus"),
        "aiff" | "aif" => Some("audio/aiff"),
        // Video
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "ogv" => Some("video/ogg"),
        "avi" => Some("video/x-msvideo"),
        "mov" => Some("video/quicktime"),
        "wmv" => Some("video/x-ms-wmv"),
        "flv" => Some("video/x-flv"),
        "mkv" => Some("video/x-matroska"),
        "mpeg" | "mpg" => Some("video/mpeg"),
        "3gp" => Some("video/3gpp"),
        // Archives
        "zip" => Some("application/zip"),
        "gz" | "gzip" => Some("application/gzip"),
        "tar" => Some("application/x-tar"),
        "bz2" => Some("application/x-bzip2"),
        "xz" => Some("application/x-xz"),
        "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/vnd.rar"),
        "zst" => Some("application/zstd"),
        "lz" => Some("application/x-lzip"),
        "lz4" => Some("application/x-lz4"),
        // Documents
        "pdf" => Some("application/pdf"),
        "doc" => Some("application/msword"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xls" => Some("application/vnd.ms-excel"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "odt" => Some("application/vnd.oasis.opendocument.text"),
        "ods" => Some("application/vnd.oasis.opendocument.spreadsheet"),
        "odp" => Some("application/vnd.oasis.opendocument.presentation"),
        "epub" => Some("application/epub+zip"),
        // Fonts
        "woff" => Some("font/woff"),
        "woff2" => Some("font/woff2"),
        "ttf" => Some("font/ttf"),
        "otf" => Some("font/otf"),
        "eot" => Some("application/vnd.ms-fontobject"),
        // Data formats
        "yaml" | "yml" => Some("application/x-yaml"),
        "toml" => Some("application/toml"),
        "ini" | "cfg" => Some("text/plain"),
        "env" => Some("text/plain"),
        "wasm" => Some("application/wasm"),
        "bin" => Some("application/octet-stream"),
        "exe" => Some("application/x-msdownload"),
        "dll" | "so" | "dylib" => Some("application/octet-stream"),
        // Package formats
        "deb" => Some("application/x-debian-package"),
        "rpm" => Some("application/x-rpm"),
        "dmg" => Some("application/x-apple-diskimage"),
        "iso" => Some("application/x-iso9660-image"),
        "whl" => Some("application/zip"),
        "egg" => Some("application/zip"),
        _ => None,
    }
}

fn mime_to_ext(mime: &str) -> Option<&'static str> {
    match mime {
        "text/html" => Some(".html"),
        "text/css" => Some(".css"),
        "text/csv" => Some(".csv"),
        "text/plain" => Some(".txt"),
        "text/markdown" => Some(".md"),
        "text/calendar" => Some(".ics"),
        "text/x-python" => Some(".py"),
        "text/x-rust" => Some(".rs"),
        "application/javascript" => Some(".js"),
        "application/json" => Some(".json"),
        "application/xml" => Some(".xml"),
        "application/pdf" => Some(".pdf"),
        "application/zip" => Some(".zip"),
        "application/gzip" => Some(".gz"),
        "application/x-tar" => Some(".tar"),
        "application/x-bzip2" => Some(".bz2"),
        "application/x-xz" => Some(".xz"),
        "application/x-7z-compressed" => Some(".7z"),
        "application/rtf" => Some(".rtf"),
        "application/sql" => Some(".sql"),
        "application/wasm" => Some(".wasm"),
        "application/x-sh" => Some(".sh"),
        "application/octet-stream" => Some(".bin"),
        "application/msword" => Some(".doc"),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => Some(".docx"),
        "application/vnd.ms-excel" => Some(".xls"),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some(".xlsx"),
        "application/epub+zip" => Some(".epub"),
        "image/jpeg" => Some(".jpg"),
        "image/png" => Some(".png"),
        "image/gif" => Some(".gif"),
        "image/bmp" => Some(".bmp"),
        "image/svg+xml" => Some(".svg"),
        "image/webp" => Some(".webp"),
        "image/x-icon" => Some(".ico"),
        "image/tiff" => Some(".tiff"),
        "image/avif" => Some(".avif"),
        "audio/mpeg" => Some(".mp3"),
        "audio/wav" => Some(".wav"),
        "audio/ogg" => Some(".ogg"),
        "audio/flac" => Some(".flac"),
        "audio/aac" => Some(".aac"),
        "audio/mp4" => Some(".m4a"),
        "audio/opus" => Some(".opus"),
        "video/mp4" => Some(".mp4"),
        "video/webm" => Some(".webm"),
        "video/ogg" => Some(".ogv"),
        "video/quicktime" => Some(".mov"),
        "video/x-msvideo" => Some(".avi"),
        "video/x-matroska" => Some(".mkv"),
        "font/woff" => Some(".woff"),
        "font/woff2" => Some(".woff2"),
        "font/ttf" => Some(".ttf"),
        "font/otf" => Some(".otf"),
        _ => None,
    }
}

pub fn create_mimetypes_module() -> PyObjectRef {
    make_module(
        "mimetypes",
        vec![
            (
                "guess_type",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("guess_type requires a url"));
                    }
                    let url = args[0].py_to_string();
                    let ext = url.rsplit('.').next().unwrap_or("");
                    let mime = ext_to_mime(ext);
                    match mime {
                        Some(m) => Ok(PyObject::tuple(vec![
                            PyObject::str_val(CompactString::from(m)),
                            PyObject::none(),
                        ])),
                        None => Ok(PyObject::tuple(vec![PyObject::none(), PyObject::none()])),
                    }
                }),
            ),
            (
                "guess_extension",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error("guess_extension requires a type"));
                    }
                    let mime = args[0].py_to_string();
                    let ext = mime_to_ext(&mime);
                    match ext {
                        Some(e) => Ok(PyObject::str_val(CompactString::from(e))),
                        None => Ok(PyObject::none()),
                    }
                }),
            ),
            (
                "guess_all_extensions",
                make_builtin(|args: &[PyObjectRef]| {
                    if args.is_empty() {
                        return Err(PyException::type_error(
                            "guess_all_extensions requires a type",
                        ));
                    }
                    let mime = args[0].py_to_string();
                    let ext = mime_to_ext(&mime);
                    match ext {
                        Some(e) => Ok(PyObject::list(vec![PyObject::str_val(
                            CompactString::from(e),
                        )])),
                        None => Ok(PyObject::list(vec![])),
                    }
                }),
            ),
            ("init", make_builtin(|_| Ok(PyObject::none()))),
            ("types_map", PyObject::dict(IndexMap::new())),
        ],
    )
}
