use axum::{
    extract::{Form, Path as AxumPath, Query, State},
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
use maud::{html, Markup, DOCTYPE}; // PreEscaped might be useful sometimes but not strictly needed here
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
    let app = Router::new()
        .route("/", get(root_handler)) // Uses Maud
        .route("/browse", get(browse_handler)) // Uses Maud
        .route("/share", post(share_handler)) // Uses Maud
        .route("/download/:uuid", get(download_handler))
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
                script src="/static/context_menu.js" defer {}
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

                // Context Menu Structure (hidden initially)
                div #context-menu {
                    ul {
                        // Target for share button/link display
                        li #context-share-target {
                             button #context-share
                                hx-post="/share"
                                hx-trigger="click"
                                hx-target="#context-share-target" // Target self for replacement
                                hx-swap="innerHTML"
                                // hx-vals will be set by JS
                                { "🔗 Share File" }
                        }
                        // Add other context actions here if needed
                        // e.g., li { "Download" }
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
    // Return Result<Markup, Response> for error handling
    let requested_path_str = query.path.unwrap_or_else(|| ".".to_string());

    // --- Security: Path Validation ---
    let sanitized_req_path = sanitize_path(&requested_path_str);
    // `resolve_and_validate_path` returns `Result<PathBuf, Response>`
    let full_path = resolve_and_validate_path(&state.root_dir, &sanitized_req_path)?;

    if !full_path.is_dir() {
        error!("Browse attempt on non-directory: {}", full_path.display());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Requested path is not a directory.",
        ));
    }

    // --- Read Directory Contents ---
    let mut entries = match fs::read_dir(&full_path).await {
        Ok(reader) => reader,
        Err(e) => {
            error!("Failed to read directory {}: {}", full_path.display(), e);
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Error reading directory contents.", // Keep sensitive info out of response
            ));
        }
    };

    let mut dir_items = Vec::new();
    let mut file_items = Vec::new();

    // Use loop instead of while let Some(Ok(entry)) = ... to handle potential errors per entry
    loop {
        match entries.next_entry().await {
            Ok(Some(entry)) => {
                let entry_path = entry.path();
                // Skip entry if getting filename fails (unlikely but possible)
                let name = match entry.file_name().into_string() {
                    Ok(n) => n,
                    Err(_) => {
                        error!(
                            "Skipping entry with non-UTF8 filename in {}",
                            full_path.display()
                        );
                        continue; // Skip this entry
                    }
                };

                // Calculate relative path for client use (URL-safe separators)
                // This unwrap is safe because full_path is canonicalized and within root_dir
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
                        error!(
                            "Failed to get metadata for {}: {}",
                            entry.path().display(),
                            e
                        );
                        // Decide whether to skip or return an error. Skipping is often better UX.
                        continue;
                    }
                }
            }
            Ok(None) => break, // End of directory stream
            Err(e) => {
                error!(
                    "Error reading directory entry in {}: {}",
                    full_path.display(),
                    e
                );
                // Decide if this error is fatal for the whole listing or just skip
                // For now, let's return an error for the request
                return Err(error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Error reading directory entry.",
                ));
            }
        }
    }

    // Sort directories and files alphabetically by name (case-insensitive)
    dir_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    file_items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // --- Generate Maud Markup ---
    let current_display_path = if sanitized_req_path == Path::new(".") {
        "/".to_string() // Display root as "/"
    } else {
        // Ensure leading slash for display consistency, normalize separators
        format!(
            "/{}",
            sanitized_req_path.to_string_lossy().replace('\\', "/")
        )
    };

    // This Markup will replace the content of #file-browser due to hx-target/hx-swap
    Ok(html! {
        // --- Current Path Display ---
        // This div *is part of* the swapped content for #file-browser
        div #current-path-container { // Use the ID expected by CSS/JS if needed
            div #current-path { "Current: " (current_display_path) }
        }

        // --- File List Container ---
        // This div also *is part of* the swapped content for #file-browser
        div #file-list-container {
            ul #file-list {
                // "Go Up" Link (if not at root)
                @if sanitized_req_path != Path::new(".") {
                    // Calculate parent path relative to root
                    @let parent_rel_path = sanitized_req_path
                        .parent()
                        .map(|p| p.to_string_lossy().replace('\\', "/")) // Normalize separators
                        .unwrap_or_else(|| ".".to_string()); // Go to root if parent is root
                    @let parent_url_encoded = urlencoding::encode(&parent_rel_path);

                    // *** CORRECTED INTERPOLATION ***
                    // Construct the hx-get value using format!
                    @let hx_get_value_up = format!("/browse?path={}", parent_url_encoded);

                    // This list item triggers navigation UPWARDS
                    li hx-get=(hx_get_value_up) // Use the formatted string directly
                       hx-target="#file-browser"                 // Target the main container
                       hx-swap="innerHTML"                       // Replace its content
                       style="cursor: pointer;" { // Add cursor pointer for better UX
                        span class="icon" { "⬆️" }
                        span { ".." }
                    }
                }

                // Directories
                @for item in &dir_items {
                    @let path_url_encoded = urlencoding::encode(&item.path);

                    // *** CORRECTED INTERPOLATION ***
                    // Construct the hx-get value using format!
                    @let hx_get_value_dir = format!("/browse?path={}", path_url_encoded);

                    // This list item triggers navigation INTO the directory
                    li data-path=(item.path) data-is-dir="true" // Data attributes for JS context menu
                       hx-get=(hx_get_value_dir) // Use the formatted string directly
                       hx-target="#file-browser"                 // Target the main container
                       hx-swap="innerHTML"                       // Replace its content
                       style="cursor: pointer;" { // Add cursor pointer
                       div {
                           span class="icon" { "📁" }
                           span { (item.name) }
                        }
                       div class="file-info" { (item.modified.as_deref().unwrap_or("")) } // Display modification time
                   }
                }

                // Files
                @for item in &file_items {
                    // Files are not directly navigable via hx-get in this setup
                    // They are interactive via the right-click context menu
                     li data-path=(item.path) data-is-dir="false" { // Data attributes for JS context menu
                         div {
                            span class="icon" { "📄" }
                            span { (item.name) }
                         }
                         div class="file-info" {
                             // Display size if available
                             @if let Some(size) = &item.size {
                                 span { (size) " " }
                             }
                             // Display modification time if available
                             @if let Some(modified) = &item.modified {
                                span { (modified) }
                             }
                             // Add more info here if needed
                         }
                    }
                }
            } // end ul
        } // end #file-list-container
    }) // end Ok(html!...)
}

/// Handles requests to create a share link for a file. Returns Maud Markup.
async fn share_handler(
    // --- ENSURE THIS IS PRESENT ---
    State(state): State<SharedState>,
    // --- AND THIS IS PRESENT ---
    Form(payload): Form<SharePayload>,
) -> Result<Markup, Response> {
    // Return Result for error handling
    // --- Now 'state' is in scope ---
    info!("Share requested for path: {}", payload.path);

    // --- Security: Path Validation ---
    let sanitized_req_path = sanitize_path(&payload.path);
    // Use 'state' here
    let full_path = resolve_and_validate_path(&state.root_dir, &sanitized_req_path)?;

    if !full_path.is_file() {
        error!("Share attempt on non-file: {}", full_path.display());
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "Sharing is only supported for files.",
        ));
    }

    // --- Generate Share Link ---
    let uuid = Uuid::new_v4();
    // Use 'state' here
    state.shares.insert(uuid, full_path.clone());

    info!("Created share link {} for {}", uuid, full_path.display());

    // Construct the URL the user will use
    let share_url = format!("/download/{}", uuid);

    // Return Maud Markup for the input field to be swapped into #context-share-target
    Ok(html! {
        div #share-link-display { // Use the same ID as CSS expects
            span { "Share Link:" }
            // Readonly input, auto-select on click is good UX
            input type="text" value=(share_url) readonly onclick="this.select();";
        }
    })
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
    // Use the stored *absolute* path for re-validation check against root.
    // Note: resolve_and_validate_path expects a *relative* path as second arg typically.
    // We need to ensure the *canonicalized absolute* path still starts with root.
    // Let's slightly adjust the check here or rely on the canonicalization done during resolve_and_validate
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
                // Optionally remove the invalid share: state.shares.remove(&uuid);
                return error_response(StatusCode::FORBIDDEN, "Access denied.");
            }
            // Ensure it's still a file
            if !canonical_path_now.is_file() {
                error!(
                    "Shared path {} is no longer a file (UUID: {}).",
                    canonical_path_now.display(),
                    uuid
                );
                // Optionally remove the invalid share: state.shares.remove(&uuid);
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
            // Optionally remove the invalid share: state.shares.remove(&uuid);
            // Treat most canonicalization errors as 'Not Found' or 'Forbidden' from client perspective
            if e.kind() == std::io::ErrorKind::NotFound {
                return error_response(StatusCode::NOT_FOUND, "Shared file not found.");
            } else {
                return error_response(StatusCode::FORBIDDEN, "Cannot access shared file.");
            }
        }
    }

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
            // Optionally remove the invalid share: state.shares.remove(&uuid);
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
