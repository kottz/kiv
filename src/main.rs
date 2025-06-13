use axum::{
    extract::{Form, Path as AxumPath, Query, State}, // Host is no longer needed here or implicitly
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
// ... (other imports remain the same)
use chrono::prelude::*;
use clap::Parser;
use dashmap::DashMap;
use humansize::{format_size, BINARY};
use maud::{html, Markup, PreEscaped, DOCTYPE};
use serde::{Deserialize, Serialize};
use std::{
    fs::Metadata,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;
use tokio_util::io::ReaderStream;
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

// --- Configuration --- (remains the same)
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    root_dir: PathBuf,
    #[arg(short, long, value_name = "ADDR", default_value = "127.0.0.1:3001")]
    bind_addr: SocketAddr,
}

// --- State --- (remains the same)
type SharedState = Arc<AppState>;
type ShareMap = DashMap<Uuid, PathBuf>;

struct AppState {
    root_dir: PathBuf,
    shares: ShareMap,
}

// --- Request Payloads --- (remains the same)
#[derive(Deserialize, Debug)]
struct BrowseQuery {
    path: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SharePayload {
    path: String,
}

#[derive(Deserialize, Debug)]
struct PreviewQuery {
    path: String,
}

// --- Response Data --- (remains the same)
#[derive(Serialize, Debug)]
struct DirEntryInfo {
    name: String,
    path: String,
    is_dir: bool,
    size: Option<String>,
    modified: Option<String>,
}

// --- Main Application --- (remains the same, including router setup)
#[tokio::main]
async fn main() {
    let args = Args::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let absolute_root_dir = match fs::canonicalize(&args.root_dir).await {
        Ok(path) => path,
        Err(e) => {
            error!(
                "Failed to resolve root directory '{}': {}. Exiting.",
                args.root_dir.display(),
                e
            );
            eprintln!(
                "Error: Failed to resolve root directory '{}': {}",
                args.root_dir.display(),
                e
            );
            std::process::exit(1);
        }
    };

    if !absolute_root_dir.is_dir() {
        error!(
            "Root path '{}' is not a directory. Exiting.",
            absolute_root_dir.display()
        );
        eprintln!(
            "Error: Root path '{}' is not a directory.",
            absolute_root_dir.display()
        );
        std::process::exit(1);
    }

    info!("Serving files from: {}", absolute_root_dir.display());
    info!("Listening on: {}", args.bind_addr);

    let shared_state = Arc::new(AppState {
        root_dir: absolute_root_dir.clone(),
        shares: DashMap::new(),
    });

    let cors = CorsLayer::new()
        .allow_methods([http::Method::GET, http::Method::POST])
        .allow_origin(Any);

    let app = Router::new()
        .route("/", get(root_handler))
        .route("/browse", get(browse_handler))
        .route("/preview", get(preview_handler))
        .route("/share", post(share_handler)) // This handler is modified
        .route("/share/{uuid}", get(share_landing_handler))
        .route("/direct-download/{uuid}", get(download_handler))
        .nest_service("/static", ServeDir::new("static"))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(shared_state);

    let listener = match tokio::net::TcpListener::bind(args.bind_addr).await {
        Ok(l) => l,
        Err(e) => {
            error!("Failed to bind to address {}: {}", args.bind_addr, e);
            eprintln!("Error: Failed to bind to address {}: {}", args.bind_addr, e);
            std::process::exit(1);
        }
    };
    if let Err(e) = axum::serve(listener, app).await {
        error!("Server error: {}", e);
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }
}

// --- root_handler --- (remains the same)
async fn root_handler() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "File Browser" }
                link rel="stylesheet" href="/static/styles.css";
                link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/styles/default.min.css";
                script src="/static/htmx.min.js" {}
                script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.11.1/highlight.min.js" {}
                script { (PreEscaped("hljs.highlightAll();")) }
                script src="/static/context_menu.js" defer {}
                script src="/static/copy_link.js" defer {}
                script {
                    (PreEscaped("
                        // Highlight syntax when HTMX swaps content
                        htmx.on('htmx:afterSwap', function(evt) {
                            console.log('HTMX afterSwap event triggered');
                            if (typeof hljs !== 'undefined') {
                                console.log('Running hljs.highlightAll()');
                                hljs.highlightAll();
                            } else {
                                console.log('hljs is undefined');
                            }
                        });
                    "))
                }
            }
            body {
                h1 { "File Browser" }
                div #file-browser
                    hx-get="/browse?path=."
                    hx-trigger="load"
                    hx-target="#file-browser"
                    hx-swap="innerHTML" {
                    div #current-path-container { "Loading path..." }
                    div #file-list-container { "Loading files..." }
                }
                div #share-result-area {}
                div #context-menu {
                    ul {
                        li #context-share-target {
                            span #context-share-button-wrapper {
                                button #context-share
                                    hx-post="/share"
                                    hx-trigger="click"
                                    hx-target="#context-share-button-wrapper"
                                    hx-swap="innerHTML"
                                    { "🔗 Share File" }
                           }
                        }
                    }
                }
            }
        }
    }
}

// --- browse_handler --- (remains the same)
async fn browse_handler(
    State(state): State<SharedState>,
    Query(query): Query<BrowseQuery>,
) -> Result<Markup, Response> {
    let requested_path_str = query.path.unwrap_or_else(|| ".".to_string());
    let sanitized_req_path = sanitize_path(&requested_path_str);
    let full_path = resolve_and_validate_path(&state.root_dir, &sanitized_req_path)?;

    if !full_path.is_dir() {
        error!("Browse attempt on non-directory: {}", full_path.display());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Requested path is not a directory.",
        ));
    }

    let mut entries = match fs::read_dir(&full_path).await {
        Ok(reader) => reader,
        Err(e) => {
            error!("Failed to read directory {}: {}", full_path.display(), e);
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error reading directory contents.",
            ));
        }
    };

    let mut dir_items = Vec::new();
    let mut file_items = Vec::new();

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_path = entry.path();
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => {
                error!(
                    "Skipping entry with non-UTF8 filename in {}",
                    full_path.display()
                );
                continue;
            }
        };

        let relative_path = entry_path
            .strip_prefix(&state.root_dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");

        match entry.metadata().await {
            Ok(metadata) => {
                let is_dir = metadata.is_dir();
                let (size, modified) = get_metadata_strings(&metadata);

                let item = DirEntryInfo {
                    name,
                    path: relative_path,
                    is_dir,
                    size,
                    modified,
                };

                if is_dir {
                    dir_items.push(item);
                } else {
                    file_items.push(item);
                }
            }
            Err(e) => {
                error!("Failed to get metadata for {}: {}", entry_path.display(), e);
                continue;
            }
        }
    }

    dir_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    file_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let current_display_path = if sanitized_req_path == Path::new(".") {
        "/".to_string()
    } else {
        format!(
            "/{}",
            sanitized_req_path.to_string_lossy().replace('\\', "/")
        )
    };

    Ok(html! {
        div #current-path-container {
            div #current-path { "Current: " (current_display_path) }
        }
        div #file-list-container {
            ul #file-list {
                @if sanitized_req_path != Path::new(".") {
                    @let parent_rel_path = sanitized_req_path.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|| ".".to_string());
                    @let parent_url_encoded = urlencoding::encode(&parent_rel_path);
                    @let hx_get_value_up = format!("/browse?path={}", parent_url_encoded);
                    li hx-get=(hx_get_value_up) hx-target="#file-browser" hx-swap="innerHTML" style="cursor: pointer;" {
                        span class="icon" { "⬆️" }
                        span { ".." }
                    }
                }
                @for item in &dir_items {
                    @let path_url_encoded = urlencoding::encode(&item.path);
                    @let hx_get_value_dir = format!("/browse?path={}", path_url_encoded);
                    li data-path=(item.path) data-is-dir="true" hx-get=(hx_get_value_dir) hx-target="#file-browser" hx-swap="innerHTML" style="cursor: pointer;" {
                       div {
                           span class="icon" { "📁" }
                           span { (item.name) }
                        }
                       div class="file-info" { (item.modified.as_deref().unwrap_or("")) }
                   }
                }
                @for item in &file_items {
                    @let item_id_base = item.path.replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
                    @let li_id = format!("file-item-{}", item_id_base);
                    @let placeholder_id = format!("share-placeholder-{}", item_id_base);
                    @let full_file_path = state.root_dir.join(&item.path);
                    @let is_previewable = is_previewable_file(&full_file_path);

                    @if is_previewable {
                        @let encoded_path = urlencoding::encode(&item.path);
                        @let preview_url = format!("/preview?path={}", encoded_path);
                        li #(li_id) data-path=(item.path) data-is-dir="false"
                           hx-get=(preview_url)
                           hx-target="#file-browser"
                           hx-swap="innerHTML"
                           style="cursor: pointer;" {
                            div {
                                span class="icon" { "📄" }
                                span { (item.name) }
                            }
                            div class="file-info" {
                                @if let Some(size) = &item.size { span { (size) " " } }
                                @if let Some(modified) = &item.modified { span { (modified) } }
                            }
                        }
                    } @else {
                        li #(li_id) data-path=(item.path) data-is-dir="false" {
                            div {
                                span class="icon" { "📄" }
                                span { (item.name) }
                            }
                            div class="file-info" {
                                @if let Some(size) = &item.size { span { (size) " " } }
                                @if let Some(modified) = &item.modified { span { (modified) } }
                            }
                        }
                    }
                    div #(placeholder_id) class="share-link-placeholder" {}
                }
            }
        }
    })
}

// --- preview_handler ---
async fn preview_handler(
    State(state): State<SharedState>,
    Query(query): Query<PreviewQuery>,
) -> Result<Markup, Response> {
    let sanitized_req_path = sanitize_path(&query.path);
    let full_path = resolve_and_validate_path(&state.root_dir, &sanitized_req_path)?;

    if !full_path.is_file() {
        error!("Preview attempt on non-file: {}", full_path.display());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Preview is only supported for files.",
        ));
    }

    // Check if file is previewable
    if !is_previewable_file(&full_path) {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "File type not supported for preview.",
        ));
    }

    // Read file content
    let content = match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => content,
        Err(e) => {
            error!(
                "Failed to read file for preview {}: {}",
                full_path.display(),
                e
            );
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read file content.",
            ));
        }
    };

    let filename = full_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown file")
        .to_string();

    let language = detect_language(&full_path);

    // Get the parent directory for the back button
    let parent_path = sanitized_req_path
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| ".".to_string());
    let encoded_parent_path = urlencoding::encode(&parent_path);
    let back_url = format!("/browse?path={}", encoded_parent_path);

    Ok(html! {
        div class="preview-container" {
            div class="preview-header" {
                h1 { "File Preview: " (filename) }
                div class="preview-actions" {
                    button hx-get=(back_url)
                           hx-target="#file-browser"
                           hx-swap="innerHTML"
                           class="close-button" { "Back to Files" }
                }
            }
            div class="preview-content" {
                pre {
                    code class=(format!("language-{}", language)) {
                        (content)
                    }
                }
            }
        }
        script {
            (PreEscaped(&format!("
                console.log('Preview content loaded for language: {}');
                console.log('hljs available:', typeof hljs !== 'undefined');
                if (typeof hljs !== 'undefined') {{
                    console.log('Calling hljs.highlightAll() from preview');
                    hljs.highlightAll();
                }}
            ", language)))
        }
    })
}

// --- MODIFIED share_handler ---
async fn share_handler(
    State(state): State<SharedState>, // App state
    // Host(hostname): Host, // Removed: We no longer extract the hostname
    Form(payload): Form<SharePayload>, // Form data (path)
) -> Result<Markup, Response> {
    info!("Share requested for path: {}", payload.path);
    // info!("Request received via host: {}", hostname); // Removed

    let sanitized_req_path = sanitize_path(&payload.path);
    let full_path = resolve_and_validate_path(&state.root_dir, &sanitized_req_path)?;

    if !full_path.is_file() {
        error!("Share attempt on non-file: {}", full_path.display());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Sharing is only supported for files.",
        ));
    }

    let uuid = Uuid::new_v4();
    state.shares.insert(uuid, full_path.clone());
    info!(
        "Created share entry for UUID {} pointing to {}",
        uuid,
        full_path.display()
    );

    // --- Construct RELATIVE URL path to the landing page ---
    // The link will be relative to the current domain, e.g., "/share/uuid-goes-here"
    let share_link_path = format!("/share/{}", uuid);
    info!("Relative share link path generated: {}", share_link_path);
    // --- End Construct URL ---

    // --- Determine Target Placeholder ID (same as before) ---
    let item_id_base = payload
        .path
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
    let target_placeholder_id = format!("share-placeholder-{}", item_id_base);
    let input_id = format!("share-link-input-{}", uuid);

    // --- Create OOB Swap Response Targeting Placeholder ---
    Ok(html! {
        // 1. Default Swap Content (targets #context-share-button-wrapper)
        { "" } // Clears the "Share File" button from the context menu span

        // 2. Out-of-Band Swap Content - The Share Link Box
        div class="share-link-inline-box"
            hx-swap-oob={"innerHTML:#"(target_placeholder_id)} // Target the specific placeholder
            {
            span { "Share Link:" }
            div style="display: flex; align-items: center; gap: 10px;" {
                input type="text"
                      id=(input_id)
                      // --- NOW USING THE RELATIVE LANDING PAGE URL PATH ---
                      value=(share_link_path) // Use the relative path
                      readonly;
                button class="copy-button"
                       data-copy-target={"#"(input_id)}
                       type="button" { "Copy" }
                button class="close-inline-share"
                        type="button"
                        onclick={"document.getElementById('"(target_placeholder_id)"').innerHTML = '';"}
                        { (PreEscaped("×")) } // Close button (cross icon)
            }
        }
    })
}

// --- share_landing_handler --- (remains the same)
async fn share_landing_handler(
    State(state): State<SharedState>,
    AxumPath(uuid): AxumPath<Uuid>,
) -> Response {
    info!("Share landing page requested for UUID: {}", uuid);

    let path_to_serve = match state.shares.get(&uuid) {
        Some(path_ref) => path_ref.value().clone(),
        None => {
            info!("Share link not found: {}", uuid);
            return error_response(StatusCode::NOT_FOUND, "Invalid or expired share link.");
        }
    };

    info!("Showing landing page for: {}", path_to_serve.display());

    match path_to_serve.canonicalize() {
        Ok(canonical_path_now) => {
            if !canonical_path_now.starts_with(&state.root_dir) {
                error!(
                    "Shared path {} resolved outside root {} for landing page (UUID: {}).",
                    path_to_serve.display(),
                    state.root_dir.display(),
                    uuid
                );
                return error_response(StatusCode::FORBIDDEN, "Access denied.");
            }
            if !canonical_path_now.is_file() {
                error!(
                    "Shared path {} is no longer a file (UUID: {}).",
                    canonical_path_now.display(),
                    uuid
                );
                return error_response(
                    StatusCode::NOT_FOUND,
                    "Shared item is no longer accessible as a file.",
                );
            }
        }
        Err(e) => {
            error!(
                "Failed to validate path {} for landing page (UUID: {}): {}",
                path_to_serve.display(),
                uuid,
                e
            );
            if e.kind() == std::io::ErrorKind::NotFound {
                return error_response(StatusCode::NOT_FOUND, "Shared file not found.");
            } else {
                return error_response(StatusCode::FORBIDDEN, "Cannot access shared file.");
            }
        }
    }

    let metadata = match tokio::fs::metadata(&path_to_serve).await {
        Ok(meta) => meta,
        Err(e) => {
            error!(
                "Failed to get metadata for shared file {}: {}",
                path_to_serve.display(),
                e
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read file information.",
            );
        }
    };

    let filename = path_to_serve
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown file")
        .to_string();

    let extension = path_to_serve
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let file_icon = match extension.as_str() {
        "pdf" => "📄",
        "doc" | "docx" => "📝",
        "xls" | "xlsx" => "📊",
        "ppt" | "pptx" => "📑",
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" => "🖼️",
        "mp3" | "wav" | "flac" | "ogg" => "🎵",
        "mp4" | "avi" | "mov" | "mkv" | "webm" => "🎬",
        "zip" | "rar" | "7z" | "tar" | "gz" => "🗄️",
        "txt" | "md" | "rst" => "📄",
        "html" | "htm" | "css" | "js" => "🌐",
        "exe" | "msi" | "dmg" | "app" => "📦",
        _ => "📄",
    };

    let (size, modified) = get_metadata_strings(&metadata);
    let mime_type = mime_guess::from_path(&path_to_serve)
        .first_or_octet_stream()
        .to_string();

    let markup = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "Download " (filename) }
                link rel="stylesheet" href="/static/styles.css"; // Relative path for CSS
            }
            body {
                div class="download-card" {
                    div class="file-header" {
                        div class="file-icon" { (file_icon) }
                        div class="file-title" { h1 { (filename) } }
                    }
                    div class="file-meta" {
                        @if let Some(size_str) = &size { div { strong { "Size:" } (size_str) } }
                        @if let Some(mod_str) = &modified { div { strong { "Modified:" } (mod_str) } }
                        div { strong { "Type:" } (mime_type) }
                    }
                    // The download link is also relative
                    a href={"/direct-download/"(uuid)} class="download-button" { "Download File" }
                    div class="footer" {
                        "This file has been shared with you securely. Click the Download button to save it to your device."
                    }
                }
            }
        }
    };
    markup.into_response()
}

// --- download_handler --- (remains the same)
async fn download_handler(
    State(state): State<SharedState>,
    AxumPath(uuid): AxumPath<Uuid>,
) -> Response {
    info!("Download requested for UUID: {}", uuid);

    let path_to_serve = match state.shares.get(&uuid) {
        Some(path_ref) => path_ref.value().clone(),
        None => {
            info!("Share link not found: {}", uuid);
            return error_response(StatusCode::NOT_FOUND, "Invalid or expired share link.");
        }
    };

    info!("Attempting to serve file: {}", path_to_serve.display());

    match path_to_serve.canonicalize() {
        Ok(canonical_path_now) => {
            if !canonical_path_now.starts_with(&state.root_dir) {
                error!(
                    "Shared path {} resolved outside root {} during download (UUID: {}).",
                    path_to_serve.display(),
                    state.root_dir.display(),
                    uuid
                );
                return error_response(StatusCode::FORBIDDEN, "Access denied.");
            }
            if !canonical_path_now.is_file() {
                error!(
                    "Shared path {} is no longer a file (UUID: {}).",
                    canonical_path_now.display(),
                    uuid
                );
                return error_response(
                    StatusCode::NOT_FOUND,
                    "Shared item is no longer accessible as a file.",
                );
            }
        }
        Err(e) => {
            error!(
                "Failed to re-validate/canonicalize shared path {} for download (UUID: {}): {}",
                path_to_serve.display(),
                uuid,
                e
            );
            if e.kind() == std::io::ErrorKind::NotFound {
                return error_response(StatusCode::NOT_FOUND, "Shared file not found.");
            } else {
                return error_response(StatusCode::FORBIDDEN, "Cannot access shared file.");
            }
        }
    }

    let metadata = match tokio::fs::metadata(&path_to_serve).await {
        Ok(meta) => meta,
        Err(e) => {
            error!(
                "Failed to get metadata for file {}: {}",
                path_to_serve.display(),
                e
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read file information for download.",
            );
        }
    };

    match tokio::fs::File::open(&path_to_serve).await {
        Ok(file) => {
            let filename = path_to_serve
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("download")
                .to_string();

            let mime_type = mime_guess::from_path(&path_to_serve)
                .first_or_octet_stream()
                .to_string();

            let stream = ReaderStream::with_capacity(file, 1 << 18); // 256KiB buffer
            let body = axum::body::Body::from_stream(stream);

            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&mime_type)
                    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
            );
            headers.insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&metadata.len().to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("0")),
            );
            headers.insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
                    .unwrap_or_else(|_| {
                        HeaderValue::from_static("attachment; filename=\"download\"")
                    }),
            );

            (StatusCode::OK, headers, body).into_response()
        }
        Err(e) => {
            error!(
                "Failed to open file for download {}: {}",
                path_to_serve.display(),
                e
            );
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not read file for download.",
            )
        }
    }
}

// --- Utility Functions --- (remain the same)
fn error_response(status_code: StatusCode, message: &str) -> Response {
    let markup = html! {
        div style="padding: 10px; border: 1px solid red; color: red; margin: 10px;" {
            h2 { "Error" }
            p { (message) }
        }
    };
    (status_code, markup).into_response()
}

fn sanitize_path(path_str: &str) -> PathBuf {
    let decoded_path =
        urlencoding::decode(path_str).map_or_else(|_| path_str.into(), |p| p.into_owned());
    let mut clean_path = PathBuf::new();
    for component in Path::new(&decoded_path).components() {
        match component {
            std::path::Component::Normal(comp) => {
                let comp_str = comp.to_string_lossy();
                // Allow current directory, or non-hidden files, or hidden files with previewable extensions
                if comp == std::ffi::OsStr::new(".")
                    || !comp_str.starts_with('.')
                    || is_previewable_file(&PathBuf::from(comp_str.as_ref()))
                {
                    if comp == std::ffi::OsStr::new(".") && !clean_path.as_os_str().is_empty() {
                        continue;
                    }
                    clean_path.push(comp);
                }
            }
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
            std::path::Component::CurDir => {
                if clean_path.as_os_str().is_empty() {
                    clean_path.push(".");
                }
            }
            std::path::Component::ParentDir => {
                clean_path.pop();
            }
        }
    }
    if clean_path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        clean_path
    }
}

fn resolve_and_validate_path(
    root_dir: &Path,
    sanitized_relative_path: &Path,
) -> Result<PathBuf, Response> {
    let mut potentially_unsafe_path = root_dir.to_path_buf();
    potentially_unsafe_path.push(sanitized_relative_path);

    match potentially_unsafe_path.canonicalize() {
        Ok(canonical_path) => {
            if canonical_path.starts_with(root_dir) {
                Ok(canonical_path)
            } else {
                error!(
                    "Path traversal attempt: Sanitized path '{}' resolved to '{}' which is outside root '{}'",
                    sanitized_relative_path.display(),
                    canonical_path.display(),
                    root_dir.display()
                );
                Err(error_response(StatusCode::FORBIDDEN, "Access denied."))
            }
        }
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => {
                info!(
                    "Path not found during canonicalization: {}",
                    potentially_unsafe_path.display()
                );
                Err(error_response(StatusCode::NOT_FOUND, "Path not found."))
            }
            _ => {
                error!(
                    "Failed to canonicalize path '{}': {}",
                    potentially_unsafe_path.display(),
                    e
                );
                Err(error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not process path.",
                ))
            }
        },
    }
}

fn get_metadata_strings(metadata: &Metadata) -> (Option<String>, Option<String>) {
    let size = if metadata.is_file() {
        Some(format_size(metadata.len(), BINARY))
    } else {
        None
    };

    let modified = metadata.modified().ok().map(|mod_time| {
        let datetime: DateTime<Local> = mod_time.into();
        datetime.format("%Y-%m-%d %H:%M").to_string()
    });

    (size, modified)
}

fn is_previewable_file(path: &Path) -> bool {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    matches!(
        extension.as_str(),
        "rs" | "py"
            | "js"
            | "ts"
            | "jsx"
            | "tsx"
            | "html"
            | "htm"
            | "css"
            | "scss"
            | "sass"
            | "less"
            | "json"
            | "xml"
            | "yaml"
            | "yml"
            | "toml"
            | "ini"
            | "cfg"
            | "conf"
            | "config"
            | "txt"
            | "md"
            | "markdown"
            | "rst"
            | "log"
            | "csv"
            | "tsv"
            | "c"
            | "cpp"
            | "cc"
            | "cxx"
            | "h"
            | "hpp"
            | "hxx"
            | "java"
            | "kt"
            | "scala"
            | "go"
            | "rb"
            | "php"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "sql"
            | "dockerfile"
            | "makefile"
            | "cmake"
            | "gradle"
            | "vue"
            | "svelte"
            | "dart"
            | "swift"
            | "r"
            | "m"
            | "mm"
            | "pl"
            | "pm"
            | "lua"
            | "ps1"
            | "psm1"
            | "psd1"
            | "tex"
            | "latex"
            | "bib"
            | "cls"
            | "sty"
    )
}

fn detect_language(path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Handle special filenames first
    if filename == "dockerfile" || filename.starts_with("dockerfile.") {
        return "dockerfile".to_string();
    }
    if filename == "makefile" || filename.starts_with("makefile.") {
        return "makefile".to_string();
    }

    // Handle extensions
    match extension.as_str() {
        "rs" => "rust",
        "py" => "python",
        "js" | "mjs" => "javascript",
        "ts" => "typescript",
        "jsx" => "javascript",
        "tsx" => "typescript",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "sass" => "sass",
        "less" => "less",
        "json" => "json",
        "xml" => "xml",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" | "conf" | "config" => "ini",
        "md" | "markdown" => "markdown",
        "rst" => "rst",
        "c" => "c",
        "cpp" | "cc" | "cxx" => "cpp",
        "h" | "hpp" | "hxx" => "c",
        "java" => "java",
        "kt" => "kotlin",
        "scala" => "scala",
        "go" => "go",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" | "zsh" | "fish" => "bash",
        "sql" => "sql",
        "vue" => "vue",
        "svelte" => "svelte",
        "dart" => "dart",
        "swift" => "swift",
        "r" => "r",
        "m" | "mm" => "objectivec",
        "pl" | "pm" => "perl",
        "lua" => "lua",
        "ps1" | "psm1" | "psd1" => "powershell",
        "tex" | "latex" => "latex",
        "csv" | "tsv" => "csv",
        _ => "plaintext",
    }
    .to_string()
}
