//! Serves the OBS browser-source overlay on localhost.
//!
//! Deliberately a hand-rolled HTTP/1.1 responder rather than a web framework:
//! it answers two GET routes on the loopback interface, and pulling in a full
//! server stack would cost more binary size than the whole feature.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    time::timeout,
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

    // OBS polls once a second over a fresh connection; a handful in flight is
    // plenty, and the cap stops anything local from exhausting tasks.
    let slots = Arc::new(Semaphore::new(16));

    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = Arc::clone(&slots).try_acquire_owned() else {
            continue; // over the cap: drop the connection
        };
        let connection_context = Arc::clone(&context);
        tokio::spawn(async move {
            let _ = handle_connection(connection_context, stream, port).await;
            drop(permit);
        });
    }
}

async fn handle_connection(context: Arc<AppContext>, mut stream: TcpStream, port: u16) -> Result<()> {
    // A client that connects and never sends a request would otherwise hold
    // its task open forever.
    let request = match timeout(Duration::from_secs(5), read_request(&mut stream)).await {
        Ok(Ok(Some(request))) => request,
        _ => return Ok(()),
    };

    // DNS-rebinding guard: a web page the streamer visits can point its own
    // hostname at 127.0.0.1, after which the browser treats this server as
    // same-origin. Such requests still carry the attacker's hostname, so only
    // serve requests addressed to the loopback address itself.
    if !is_loopback_host(request.host.as_deref(), port) {
        return write_response(&mut stream, "403 Forbidden", "text/plain; charset=utf-8", b"Forbidden").await;
    }

    match request.path.split('?').next().unwrap_or("/") {
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

struct Request {
    path: String,
    host: Option<String>,
}

fn is_loopback_host(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else {
        return false;
    };
    let host = host.trim().to_ascii_lowercase();
    [
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        "127.0.0.1".to_string(),
        "localhost".to_string(),
    ]
    .contains(&host)
}

/// Reads the request line and the Host header. Requests here are plain GETs
/// from a browser source, so everything else is discarded.
async fn read_request(stream: &mut TcpStream) -> Result<Option<Request>> {
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

    let host = text.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("host")
            .then(|| value.trim().to_string())
    });

    Ok(Some(Request {
        path: path.to_string(),
        host,
    }))
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
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_only_requests_addressed_to_loopback() {
        assert!(is_loopback_host(Some("127.0.0.1:4747"), 4747));
        assert!(is_loopback_host(Some("localhost:4747"), 4747));
        assert!(is_loopback_host(Some("LOCALHOST:4747"), 4747));
        // DNS rebinding: an attacker hostname resolved to 127.0.0.1.
        assert!(!is_loopback_host(Some("evil.example:4747"), 4747));
        assert!(!is_loopback_host(Some("127.0.0.1:9999"), 4747));
        assert!(!is_loopback_host(None, 4747));
    }
}
