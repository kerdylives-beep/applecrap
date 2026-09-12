//! Serves the OBS browser-source overlay on localhost.
//!
//! Deliberately a hand-rolled HTTP/1.1 responder rather than a web framework:
//! it answers two GET routes on the loopback interface, and pulling in a full
//! server stack would cost more binary size than the whole feature.

use std::sync::Arc;

use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::{app::AppContext, models::LogLevel};

const OVERLAY_HTML: &str = include_str!("../scripts/overlay.html");

/// Accept loop. Runs until the task is aborted (settings change or shutdown).
pub async fn serve(context: Arc<AppContext>, port: u16) {
    // Loopback only: nothing about this should be reachable from the network,
    // and binding localhost also avoids a Windows firewall prompt.
    let listener = match TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(error) => {
            context
                .add_log(
                    LogLevel::Warn,
                    format!(
                        "Overlay server could not start on port {port}: {error}. Pick another port in the Overlay panel."
                    ),
                )
                .await;
            return;
        }
    };

    context
        .add_log(
            LogLevel::Info,
            format!("Overlay available at http://127.0.0.1:{port}/ — add it to OBS as a Browser source."),
        )
        .await;

    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let connection_context = Arc::clone(&context);
        tokio::spawn(async move {
            let _ = handle_connection(connection_context, stream).await;
        });
    }
}

async fn handle_connection(context: Arc<AppContext>, mut stream: TcpStream) -> Result<()> {
    let path = match read_request_path(&mut stream).await? {
        Some(path) => path,
        None => return Ok(()),
    };

    match path.split('?').next().unwrap_or("/") {
        "/" | "/index.html" => {
            write_response(&mut stream, "200 OK", "text/html; charset=utf-8", OVERLAY_HTML.as_bytes())
                .await
        }
        "/state" | "/state.json" => {
            let state = context.overlay_state().await;
            let body = serde_json::to_vec(&state)?;
            write_response(&mut stream, "200 OK", "application/json; charset=utf-8", &body).await
        }
        _ => write_response(&mut stream, "404 Not Found", "text/plain; charset=utf-8", b"Not found").await,
    }
}

/// Reads just enough of the request to get the path. Requests here are plain
/// GETs from a browser source, so headers are read and discarded.
async fn read_request_path(stream: &mut TcpStream) -> Result<Option<String>> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];

    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..read]);

        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        // A browser-source GET is never this large; stop rather than buffer.
        if buffer.len() > 8192 {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buffer);
    let Some(request_line) = text.lines().next() else {
        return Ok(None);
    };
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or("/");

    if method != "GET" {
        return Ok(None);
    }

    Ok(Some(path.to_string()))
}

async fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store, max-age=0\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}
