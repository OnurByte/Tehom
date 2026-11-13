//! A Chromium-based browser with its own profile, driven over the Chrome
//! DevTools Protocol. The person signs in to a platform in that window; the
//! requests the platform signs itself are made by its own page.
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The browser profile is not signed in to the platform (any more).
    LoginRequired,
    /// The platform or the setup refused; the text says why.
    Rejected(String),
    Server(u16),
    Transport(String),
    Protocol(String),
    File(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoginRequired => write!(f, "sign-in required"),
            Self::Rejected(message) => write!(f, "{message}"),
            Self::Server(code) => write!(f, "server error (HTTP {code})"),
            Self::Transport(message) => write!(f, "browser or connection problem: {message}"),
            Self::Protocol(message) => write!(f, "unexpected answer: {message}"),
            Self::File(message) => write!(f, "could not read the video: {message}"),
        }
    }
}
impl std::error::Error for Error {}
impl Error {
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Server(_))
    }
}
pub type Result<T> = std::result::Result<T, Error>;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn transport(error: impl ToString) -> Error {
    Error::Transport(error.to_string())
}

/// The first Chromium-based browser installed here.
pub fn find_browser() -> Option<PathBuf> {
    let names = [
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
        "microsoft-edge-stable",
        "microsoft-edge",
        "brave-browser",
        "brave",
        "chrome",
    ];
    let fixed = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ];
    let mut candidates: Vec<PathBuf> = fixed.iter().map(PathBuf::from).collect();
    for root in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(root) = std::env::var_os(root) {
            for tail in [
                r"Google\Chrome\Application\chrome.exe",
                r"Microsoft\Edge\Application\msedge.exe",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
                r"Chromium\Application\chrome.exe",
            ] {
                candidates.push(Path::new(&root).join(tail));
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            for name in names {
                candidates.push(dir.join(name));
            }
        }
    }
    candidates.into_iter().find(|path| path.is_file())
}

pub struct Chromium {
    child: Child,
    port: u16,
}

impl Chromium {
    /// Opens a window on `url` with `profile` as its own data directory.
    pub async fn launch(profile: &Path, url: &str) -> Result<Self> {
        let browser = find_browser().ok_or_else(|| {
            Error::Rejected(
                "No Chrome, Chromium, Edge or Brave browser was found; install one to use this"
                    .into(),
            )
        })?;
        tokio::fs::create_dir_all(profile)
            .await
            .map_err(|error| Error::File(error.to_string()))?;
        let marker = profile.join("DevToolsActivePort");
        let _ = tokio::fs::remove_file(&marker).await;
        let child = Command::new(browser)
            .arg(format!("--user-data-dir={}", profile.display()))
            .args([
                "--remote-debugging-port=0",
                "--no-first-run",
                "--no-default-browser-check",
            ])
            .arg(url)
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| Error::Rejected(format!("Could not start the browser: {error}")))?;
        for _ in 0..200 {
            if let Some(port) = tokio::fs::read_to_string(&marker)
                .await
                .ok()
                .and_then(|text| text.lines().next()?.trim().parse().ok())
            {
                return Ok(Self { child, port });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::Rejected(
            "The browser did not start; close any window already using its profile and try again"
                .into(),
        ))
    }
    /// The window's first tab.
    pub async fn page(&self) -> Result<Page> {
        for _ in 0..100 {
            let tabs: Value = reqwest::get(format!("http://127.0.0.1:{}/json/list", self.port))
                .await
                .map_err(transport)?
                .json()
                .await
                .map_err(transport)?;
            if let Some(url) = tabs
                .as_array()
                .into_iter()
                .flatten()
                .find(|tab| tab["type"] == "page")
                .and_then(|tab| tab["webSocketDebuggerUrl"].as_str())
            {
                let (socket, _) = connect_async(url).await.map_err(transport)?;
                return Ok(Page { socket, next: 0 });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::Protocol("the browser opened no tab".into()))
    }
    /// Closes the browser so its profile is written to disk.
    pub async fn close(mut self) {
        let version = async {
            let version: Value =
                reqwest::get(format!("http://127.0.0.1:{}/json/version", self.port))
                    .await
                    .ok()?
                    .json()
                    .await
                    .ok()?;
            let (mut socket, _) = connect_async(version["webSocketDebuggerUrl"].as_str()?)
                .await
                .ok()?;
            socket
                .send(Message::text(
                    json!({"id": 1, "method": "Browser.close"}).to_string(),
                ))
                .await
                .ok()
        };
        let _ = version.await;
        if tokio::time::timeout(Duration::from_secs(10), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.kill().await;
        }
    }
}

pub struct Page {
    socket: Socket,
    next: u64,
}

impl Page {
    #[cfg(test)]
    pub fn new(socket: Socket) -> Self {
        Self { socket, next: 0 }
    }
    pub async fn call(&mut self, method: &str, params: Value, wait: Duration) -> Result<Value> {
        self.next += 1;
        let id = self.next;
        self.socket
            .send(Message::text(
                json!({"id": id, "method": method, "params": params}).to_string(),
            ))
            .await
            .map_err(transport)?;
        tokio::time::timeout(wait, async {
            loop {
                let Message::Text(text) = self
                    .socket
                    .next()
                    .await
                    .ok_or_else(|| Error::Transport("the browser window was closed".into()))?
                    .map_err(transport)?
                else {
                    continue;
                };
                let Ok(answer) = serde_json::from_str::<Value>(text.as_str()) else {
                    continue;
                };
                if answer["id"] != id {
                    continue; // an event
                }
                if let Some(error) = answer.get("error") {
                    return Err(Error::Protocol(
                        error["message"].as_str().unwrap_or("browser error").into(),
                    ));
                }
                return Ok(answer["result"].clone());
            }
        })
        .await
        .map_err(|_| Error::Transport(format!("the browser did not answer {method} in time")))?
    }
    /// Runs a JavaScript expression (promises are awaited) and returns its value.
    pub async fn eval(&mut self, expression: &str, wait: Duration) -> Result<Value> {
        let result = self
            .call(
                "Runtime.evaluate",
                json!({"expression": expression, "awaitPromise": true, "returnByValue": true}),
                wait,
            )
            .await?;
        if let Some(exception) = result.get("exceptionDetails") {
            return Err(Error::Protocol(
                exception["exception"]["description"]
                    .as_str()
                    .unwrap_or("script error")
                    .into(),
            ));
        }
        Ok(result["result"]["value"].clone())
    }
    /// Waits until the page has loaded and its scripts had a moment to start.
    pub async fn settled(&mut self) -> Result<()> {
        for _ in 0..60 {
            if self
                .eval("document.readyState", Duration::from_secs(10))
                .await?
                == "complete"
            {
                // shortcut: a fixed pause for the site's scripts; upgrade if requests go out unsigned.
                tokio::time::sleep(Duration::from_secs(4)).await;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Err(Error::Transport("the page did not finish loading".into()))
    }
    /// A request made by the page itself, with the site's cookies; the
    /// site's own scripts sign it exactly as for any request the person makes.
    pub async fn fetch(
        &mut self,
        method: &str,
        url: &str,
        headers: &[(&str, String)],
        body: Option<&str>,
    ) -> Result<(u16, Value)> {
        let request = json!({
            "method": method,
            "url": url,
            "headers": headers.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
            "body": body,
        });
        let script = format!(
            "(async r => {{ const o = {{method: r.method, headers: r.headers, credentials: 'include'}}; \
             if (r.body !== null) o.body = r.body; const a = await fetch(r.url, o); \
             return {{status: a.status, text: await a.text()}}; }})({request})"
        );
        let answer = self.eval(&script, Duration::from_secs(120)).await?;
        let status = answer["status"].as_u64().unwrap_or(0) as u16;
        let text = answer["text"].as_str().unwrap_or_default();
        if status >= 500 {
            return Err(Error::Server(status));
        }
        Ok((status, serde_json::from_str(text).unwrap_or(Value::Null)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// A stand-in DevTools endpoint: answers `Runtime.evaluate` with the
    /// expression's length and sends an event first, as a real browser does.
    async fn fake() -> Page {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let request: Value = serde_json::from_str(text.as_str()).unwrap();
                socket
                    .send(Message::text(
                        json!({"method": "Page.frameNavigated"}).to_string(),
                    ))
                    .await
                    .unwrap();
                let answer = if request["method"] == "Runtime.evaluate" {
                    let script = request["params"]["expression"].as_str().unwrap();
                    if script.contains("boom") {
                        json!({"id": request["id"], "result": {"exceptionDetails": {"exception": {"description": "ReferenceError: boom"}}}})
                    } else if script.contains("fetch(") {
                        json!({"id": request["id"], "result": {"result": {"value": {"status": 200, "text": "{\"ok\":true}"}}}})
                    } else {
                        json!({"id": request["id"], "result": {"result": {"value": script.len()}}})
                    }
                } else {
                    json!({"id": request["id"], "error": {"message": "unknown method"}})
                };
                socket
                    .send(Message::text(answer.to_string()))
                    .await
                    .unwrap();
            }
        });
        let (socket, _) = connect_async(format!("ws://{address}")).await.unwrap();
        Page::new(socket)
    }

    #[tokio::test]
    async fn calls_skip_events_and_report_script_and_protocol_errors() {
        let mut page = fake().await;
        let wait = Duration::from_secs(5);
        assert_eq!(page.eval("1+1", wait).await.unwrap(), 3);
        assert_eq!(
            page.eval("boom()", wait).await.unwrap_err(),
            Error::Protocol("ReferenceError: boom".into())
        );
        assert_eq!(
            page.call("Nope.nothing", json!({}), wait)
                .await
                .unwrap_err(),
            Error::Protocol("unknown method".into())
        );
    }
    /// Needs an installed browser and a display: `cargo test -p tehom-tiktok -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn drives_a_real_browser() {
        let profile = tempfile::tempdir().unwrap();
        let browser = Chromium::launch(profile.path(), "data:text/html,<title>hi</title>")
            .await
            .unwrap();
        let mut page = browser.page().await.unwrap();
        page.settled().await.unwrap();
        let wait = Duration::from_secs(10);
        assert_eq!(page.eval("document.title", wait).await.unwrap(), "hi");
        let cookies = page
            .call("Network.getAllCookies", json!({}), wait)
            .await
            .unwrap();
        assert!(cookies["cookies"].is_array());
        browser.close().await;
        assert!(profile.path().join("Default").exists());
    }

    #[tokio::test]
    async fn fetch_returns_status_and_parsed_json() {
        let mut page = fake().await;
        let (status, body) = page
            .fetch(
                "GET",
                "https://www.tiktok.com/x",
                &[("accept", "*/*".into())],
                None,
            )
            .await
            .unwrap();
        assert_eq!((status, body["ok"].as_bool()), (200, Some(true)));
    }
}
