use axum::{
    extract::{Form, Host, Path as AxumPath, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use chrono::prelude::*;
use clap::Parser;
use dashmap::DashMap;
use humansize::{format_size, BINARY};
// --- Add Maud imports ---
use maud::{html, Markup, PreEscaped, DOCTYPE}; // PreEscaped might be useful sometimes but not strictly needed here
use serde::{Deserialize, Serialize};
use std::{
    fs::Metadata,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;
use tokio_util::io::ReaderStream; // Import for download streaming
use tower_http::{
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

// --- Configuration ---
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The root directory to serve files from
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    root_dir: PathBuf,

    /// The network address to bind to
    #[arg(short, long, value_name = "ADDR", default_value = "127.0.0.1:3000")]
    bind_addr: SocketAddr,
}

// --- State ---
type SharedState = Arc<AppState>;
type ShareMap = DashMap<Uuid, PathBuf>;

struct AppState {
    root_dir: PathBuf,
    shares: ShareMap,
}

// --- Request Payloads ---
#[derive(Deserialize, Debug)]
struct BrowseQuery {
    path: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SharePayload {
    path: String,
}

// --- Response Data ---
#[derive(Serialize, Debug)] // Keep Serialize if you might want JSON endpoints later
struct DirEntryInfo {
    name: String,
    path: String, // Relative path from root_dir for client use
    is_dir: bool,
    size: Option<String>,     // Human-readable size
    modified: Option<String>, // Formatted modification time
}

// --- Main Application ---
#[tokio::main]
async fn main() {
    // --- Setup (Args parsing, Tracing, Root Dir validation) ---
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
            // Consider using a more user-friendly error mechanism if this were a library
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

    // Create shared state
    let shared_state = Arc::new(AppState {
        root_dir: absolute_root_dir.clone(),
        shares: DashMap::new(),
    });

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_methods([http::Method::GET, http::Method::POST])
        .allow_origin(Any); // Allow any origin for simplicity, restrict in production

    // Build the Axum router
    // Update the router in the main function to include the new routes:
    let app = Router::new()
        .route("/", get(root_handler)) // Uses Maud
        .route("/browse", get(browse_handler)) // Uses Maud
        .route("/share", post(share_handler)) // Uses Maud
        .route("/share/:uuid", get(share_landing_handler)) // New landing page handler
        .route("/direct-download/:uuid", get(download_handler)) // Renamed direct download route
        // Serve static files (CSS, JS)
        .nest_service("/static", ServeDir::new("static"))
        // Add middleware
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        // Provide state to handlers
        .with_state(shared_state);

    // Run the server
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

// --- Handlers using Maud ---

/// Serves the main HTML page using Maud.
async fn root_handler() -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "File Browser" }
                link rel="stylesheet" href="/static/styles.css";
                script src="/static/htmx.min.js" defer {}
                // Script for the right-click context menu
                script src="/static/context_menu.js" defer {}
                // Script for the copy-to-clipboard functionality
                script src="/static/copy_link.js" defer {}
            }
            body {
                h1 { "File Browser" }

                // The main container for the browser view
                div #file-browser
                    hx-get="/browse?path=." // Initial load targets the root
                    hx-trigger="load"       // Trigger on page load
                    hx-target="#file-browser" // Replace this whole div on subsequent navigation
                    hx-swap="innerHTML" {

                    // Placeholders that will be replaced by the initial /browse load
                    div #current-path-container { "Loading path..." }
                    div #file-list-container { "Loading files..." }
                }

                // Dedicated area for displaying the share link result via OOB swap
                div #share-result-area {
                    // Content will be swapped in here by the /share handler
                }

                // Context Menu Structure (hidden initially)
                div #context-menu {
                    ul {
                        // The LI is the main container for the share action
                        li #context-share-target {
                            // --- ADD SPAN WRAPPER ---
                            // This span will be the target for the default swap
                            span #context-share-button-wrapper {
                                // The initial button lives inside the span
                                button #context-share
                                    hx-post="/share"
                                    hx-trigger="click"
                                    // --- CHANGE DEFAULT TARGET ---
                                    hx-target="#context-share-button-wrapper" // Target the span
                                    hx-swap="innerHTML" // Replace span content (the button)
                                    // hx-vals set by context_menu.js
                                    { "🔗 Share File" }
                           } // --- END SPAN WRAPPER ---
                        }
                    }
                }
            }
        }
    }
}

/// Handles requests to browse directory contents. Returns Maud Markup.
async fn browse_handler(
    State(state): State<SharedState>,
    Query(query): Query<BrowseQuery>,
) -> Result<Markup, Response> {
    let requested_path_str = query.path.unwrap_or_else(|| ".".to_string());

    // --- Security: Path Validation ---
    let sanitized_req_path = sanitize_path(&requested_path_str);
    let full_path = resolve_and_validate_path(&state.root_dir, &sanitized_req_path)?;

    if !full_path.is_dir() {
        error!("Browse attempt on non-directory: {}", full_path.display());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Requested path is not a directory.",
        ));
    }

    // --- Read Directory Contents (Using the simpler, working approach) ---
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

    // Use while let Some(Ok(entry)) for cleaner iteration if errors per entry are acceptable to skip
    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_path = entry.path();
        let name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => {
                error!(
                    "Skipping entry with non-UTF8 filename in {}",
                    full_path.display()
                );
                continue; // Skip this entry if filename isn't valid UTF-8
            }
        };

        // Calculate relative path
        let relative_path = entry_path
            .strip_prefix(&state.root_dir)
            .unwrap() // Safe due to prior validation
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
                // Skip entries where metadata fails
                continue;
            }
        }
    }

    // Sort directories and files alphabetically
    dir_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    file_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // --- Generate Maud Markup ---
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
                // "Go Up" Link (Correct logic from working version)
                @if sanitized_req_path != Path::new(".") {
                    @let parent_rel_path = sanitized_req_path.parent().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|| ".".to_string());
                    @let parent_url_encoded = urlencoding::encode(&parent_rel_path);
                    @let hx_get_value_up = format!("/browse?path={}", parent_url_encoded);
                    li hx-get=(hx_get_value_up) hx-target="#file-browser" hx-swap="innerHTML" style="cursor: pointer;" {
                        span class="icon" { "⬆️" }
                        span { ".." }
                    }
                }

                // Directories (Correct logic from working version)
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

                // --- Files (Add ID and Placeholder DIV to working file logic) ---
                @for item in &file_items {
                    // Create unique IDs
                    @let item_id_base = item.path.replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
                    @let li_id = format!("file-item-{}", item_id_base);
                    @let placeholder_id = format!("share-placeholder-{}", item_id_base);

                     // Add id attribute here
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
                    // Add the placeholder div immediately after the li
                    div #(placeholder_id) class="share-link-placeholder" {}
                }
            } // end ul
        } // end #file-list-container
    }) // end Ok(html!...)
}

async fn share_handler(
    State(state): State<SharedState>,  // App state
    Host(hostname): Host,              // Extract hostname (e.g., "localhost:3000")
    Form(payload): Form<SharePayload>, // Form data (path)
) -> Result<Markup, Response> {
    info!("Share requested for path: {}", payload.path);
    info!("Request received via host: {}", hostname); // Log the detected host

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
    info!("Created share link {} for {}", uuid, full_path.display());

    // --- Construct Full URL to the landing page (not direct download) ---
    let scheme = "http"; // TODO: Make configurable or add HTTPS detection if needed
    let base_url = format!("{}://{}", scheme, hostname);
    let relative_url = format!("/share/{}", uuid); // This now points to the landing page
    let full_share_url = format!("{}{}", base_url, relative_url); // Combine base + relative
    info!("Full share URL generated: {}", full_share_url);
    // --- End Construct Full URL ---

    // --- Determine Target Placeholder ID (same as before) ---
    // Ensure this logic exactly matches the one in browse_handler for ID generation
    let item_id_base = payload
        .path
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "_");
    let target_placeholder_id = format!("share-placeholder-{}", item_id_base);
    let input_id = format!("share-link-input-{}", uuid);

    // --- Create OOB Swap Response Targeting Placeholder ---
    Ok(html! {
        // 1. Default Swap Content (targets #context-share-button-wrapper)
        //    Still empty to remove the button from the span.
        { "" }

        // 2. Out-of-Band Swap Content - The Share Link Box
        div class="share-link-inline-box" // Class for styling
            hx-swap-oob={"innerHTML:#"(target_placeholder_id)} // TARGET THE SPECIFIC PLACEHOLDER
            {
            span { "Share Link:" } // Label
            div style="display: flex; align-items: center; gap: 10px;" { // Inner flex container
                input type="text"
                      id=(input_id)
                      // --- NOW USING THE LANDING PAGE URL ---
                      value=(full_share_url)
                      readonly;
                button class="copy-button"
                       data-copy-target={"#"(input_id)}
                       type="button" { "Copy" }
                // Close button
                 button class="close-inline-share"
                        type="button"
                        // JS to clear the content of the placeholder div
                        onclick={"document.getElementById('"(target_placeholder_id)"').innerHTML = '';"}
                        { (PreEscaped("×")) }
            }
        }
    })
}

async fn share_landing_handler(
    State(state): State<SharedState>,
    AxumPath(uuid): AxumPath<Uuid>,
) -> Response {
    info!("Share landing page requested for UUID: {}", uuid);

    // --- Look up the shared path ---
    let path_to_serve = match state.shares.get(&uuid) {
        Some(path_ref) => path_ref.value().clone(),
        None => {
            info!("Share link not found: {}", uuid);
            return error_response(StatusCode::NOT_FOUND, "Invalid or expired share link.");
        }
    };

    info!("Showing landing page for: {}", path_to_serve.display());

    // --- Security: Re-validate the path ---
    match path_to_serve.canonicalize() {
        Ok(canonical_path_now) => {
            // Ensure it's still within the root directory
            if !canonical_path_now.starts_with(&state.root_dir) {
                error!(
                    "Shared path {} resolved outside root {} for landing page (UUID: {}).",
                    path_to_serve.display(),
                    state.root_dir.display(),
                    uuid
                );
                return error_response(StatusCode::FORBIDDEN, "Access denied.");
            }
            // Ensure it's still a file
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

    // --- Gather file metadata ---
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

    // Extract filename
    let filename = path_to_serve
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown file")
        .to_string();

    // Extract file extension for icon display
    let extension = path_to_serve
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase();

    // Determine icon based on file extension
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
        _ => "📄", // Default file icon
    };

    // Get size and modification time
    let (size, modified) = get_metadata_strings(&metadata);

    // Get mime type
    let mime_type = mime_guess::from_path(&path_to_serve)
        .first_or_octet_stream()
        .to_string();

    // Generate the landing page markup
    let markup = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "Download " (filename) }
                link rel="stylesheet" href="/static/styles.css";
            }
            body {
                div class="download-card" {
                    div class="file-header" {
                        div class="file-icon" { (file_icon) }
                        div class="file-title" {
                            h1 { (filename) }
                        }
                    }

                    div class="file-meta" {
                        @if let Some(size_str) = &size {
                            div { strong { "Size:" } (size_str) }
                        }
                        @if let Some(mod_str) = &modified {
                            div { strong { "Modified:" } (mod_str) }
                        }
                        div { strong { "Type:" } (mime_type) }
                    }

                    a href={"/direct-download/"(uuid)} class="download-button" {
                        "Download File"
                    }

                    div class="footer" {
                        "This file has been shared with you securely. Click the Download button to save it to your device."
                    }
                }
            }
        }
    };

    // Return the landing page
    markup.into_response()
}

/// Handles requests to download a shared file via its UUID.
async fn download_handler(
    State(state): State<SharedState>,
    AxumPath(uuid): AxumPath<Uuid>,
) -> Response {
    // Returns Response directly (success or error)
    info!("Download requested for UUID: {}", uuid);

    // --- Look up the shared path ---
    let path_to_serve = match state.shares.get(&uuid) {
        Some(path_ref) => path_ref.value().clone(), // Clone the PathBuf stored in the DashMap
        None => {
            info!("Share link not found: {}", uuid);
            return error_response(StatusCode::NOT_FOUND, "Invalid or expired share link.");
        }
    };

    info!("Attempting to serve file: {}", path_to_serve.display());

    // --- Security: Re-validate the path before serving ---
    // Ensures the file hasn't been moved outside root or deleted *after* link creation.
    match path_to_serve.canonicalize() {
        // Re-canonicalize to check current state
        Ok(canonical_path_now) => {
            // Ensure it's still within the *canonicalized* root directory
            if !canonical_path_now.starts_with(&state.root_dir) {
                error!(
                    "Shared path {} resolved outside root {} during download (UUID: {}).",
                    path_to_serve.display(),
                    state.root_dir.display(),
                    uuid
                );
                return error_response(StatusCode::FORBIDDEN, "Access denied.");
            }
            // Ensure it's still a file
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
            // Path seems okay to serve *now*
        }
        Err(e) => {
            error!(
                "Failed to re-validate/canonicalize shared path {} for download (UUID: {}): {}",
                path_to_serve.display(),
                uuid,
                e
            );
            // Treat most canonicalization errors as 'Not Found' or 'Forbidden' from client perspective
            if e.kind() == std::io::ErrorKind::NotFound {
                return error_response(StatusCode::NOT_FOUND, "Shared file not found.");
            } else {
                return error_response(StatusCode::FORBIDDEN, "Cannot access shared file.");
            }
        }
    }

    // --- Get the file metadata first to determine Content-Length ---
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

    // --- Serve the file ---
    match tokio::fs::File::open(&path_to_serve).await {
        Ok(file) => {
            // Attempt to get filename for Content-Disposition
            let filename = path_to_serve
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("download") // Fallback filename
                .to_string(); // Ensure owned string

            // Guess mime type for Content-Type
            let mime_type = mime_guess::from_path(&path_to_serve)
                .first_or_octet_stream() // Default to octet-stream if guess fails
                .to_string();

            // Stream the file content efficiently
            let stream = ReaderStream::new(file);
            let body = axum::body::Body::from_stream(stream);

            // Set headers for download
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(&mime_type) // Parse guessed mime type
                    .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")), // Fallback
            );

            // Add Content-Length header with the file size
            headers.insert(
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&metadata.len().to_string())
                    .unwrap_or_else(|_| HeaderValue::from_static("0")),
            );

            headers.insert(
                header::CONTENT_DISPOSITION,
                // Properly format for attachment, handling potential quotes in filename if necessary later
                HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
                    .unwrap_or_else(|_| {
                        HeaderValue::from_static("attachment; filename=\"download\"")
                    }), // Fallback
            );

            // Return success response with headers and streamed body
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
                "Could not read file for download.", // Generic error to client
            )
        }
    }
}

// --- Utility Functions ---

/// Helper to generate consistent Maud-based error responses.
fn error_response(status_code: StatusCode, message: &str) -> Response {
    let markup = html! {
        // Simple error display, could be enhanced
        div style="padding: 10px; border: 1px solid red; color: red; margin: 10px;" {
            h2 { "Error" }
            p { (message) }
        }
    };
    (status_code, markup).into_response()
}

/// Cleans up a path string, removing potential traversal attempts.
/// Returns a PathBuf relative to *some* base (doesn't guarantee it's under root_dir yet).
fn sanitize_path(path_str: &str) -> PathBuf {
    // Decode URL encoding first
    let decoded_path =
        urlencoding::decode(path_str).map_or_else(|_| path_str.into(), |p| p.into_owned());
    let mut clean_path = PathBuf::new();
    // Process each component of the path
    for component in Path::new(&decoded_path).components() {
        match component {
            std::path::Component::Normal(comp) => {
                // Disallow components starting with '.' (like .git, .env, etc.)
                // Allow '.' itself only if it's the *first* component
                if !comp.to_string_lossy().starts_with('.') || comp == std::ffi::OsStr::new(".") {
                    if comp == std::ffi::OsStr::new(".") && !clean_path.as_os_str().is_empty() {
                        // Ignore '.' if it's not the first component (e.g., some/./dir)
                        continue;
                    }
                    clean_path.push(comp);
                } else {
                    // Log or handle disallowed hidden files/dirs if needed
                    // info!("Ignoring hidden component: {:?}", comp);
                }
            }
            // Ignore RootDir, CurDir, Prefix entirely as we build relative to our root
            std::path::Component::RootDir | std::path::Component::Prefix(_) => {}
            std::path::Component::CurDir => {
                // Allow '.' only if it's the very first component
                if clean_path.as_os_str().is_empty() {
                    clean_path.push(".");
                }
            }
            std::path::Component::ParentDir => {
                // Handle '..' by popping the last component, preventing climbing up *before* joining with root
                clean_path.pop();
            }
        }
    }
    // If the path becomes empty after sanitization (e.g., input was "/"), default to "."
    if clean_path.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        clean_path
    }
}

/// Resolves the sanitized relative path against the root directory and validates it.
/// Ensures the final path is *within* the root directory using canonicalization.
/// Returns Ok(absolute_canonical_path) or Err(axum_response).
fn resolve_and_validate_path(
    root_dir: &Path, // Should be the absolute, canonicalized root path
    sanitized_relative_path: &Path,
) -> Result<PathBuf, Response> {
    // Join the root dir with the sanitized *relative* path.
    let mut potentially_unsafe_path = root_dir.to_path_buf();
    potentially_unsafe_path.push(sanitized_relative_path);

    // --- Critical Security Step: Canonicalize and Verify ---
    // Canonicalize resolves symlinks and "."/".." components based on the *filesystem*.
    match potentially_unsafe_path.canonicalize() {
        Ok(canonical_path) => {
            // Check if the resulting canonical path still starts with the canonical root directory path.
            // This is the main defense against escaping the root via symlinks or tricky ".." combinations
            // that might bypass the initial sanitization.
            if canonical_path.starts_with(root_dir) {
                // Path is confirmed to be within the root directory bounds
                Ok(canonical_path)
            } else {
                // Path traversal attempt detected! Log and deny access.
                error!(
                    "Path traversal attempt: Sanitized path '{}' resolved to '{}' which is outside root '{}'",
                    sanitized_relative_path.display(),
                    canonical_path.display(),
                    root_dir.display()
                );
                Err(error_response(StatusCode::FORBIDDEN, "Access denied."))
            }
        }
        Err(e) => {
            // Handle errors during canonicalization (e.g., path does not exist).
            match e.kind() {
                std::io::ErrorKind::NotFound => {
                    info!(
                        "Path not found during canonicalization: {}",
                        potentially_unsafe_path.display()
                    );
                    Err(error_response(StatusCode::NOT_FOUND, "Path not found."))
                }
                // Optional: Handle permission errors specifically if needed
                // std::io::ErrorKind::PermissionDenied => { ... }
                _ => {
                    // Log unexpected errors and return a generic server error.
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
            }
        }
    }
}

/// Extracts human-readable size and modification time from metadata.
fn get_metadata_strings(metadata: &Metadata) -> (Option<String>, Option<String>) {
    // Get size only for files
    let size = if metadata.is_file() {
        Some(format_size(metadata.len(), BINARY)) // Using BINARY (KiB, MiB)
    } else {
        None // No size shown for directories
    };

    // Get modification time
    let modified = metadata.modified().ok().map(|mod_time| {
        // Convert SystemTime to DateTime<Local> for local timezone formatting
        let datetime: DateTime<Local> = mod_time.into();
        // Format as YYYY-MM-DD HH:MM
        datetime.format("%Y-%m-%d %H:%M").to_string()
    });

    (size, modified)
}
