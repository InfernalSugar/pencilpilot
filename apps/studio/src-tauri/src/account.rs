//! Signing in with a PencilPilot account, for drawing together (ADR-0013, "Sign-in and
//! passes"). Solo work never needs it and never connects.
//!
//! **Linking** (the usual way for desktop apps, RFC 8252 with PKCE): the shell listens on
//! `http://127.0.0.1:{a free port}/linked`, opens the system browser at the website's
//! `/app/link` with that address, a random state and a code challenge (the SHA-256 of a
//! secret kept here), and waits. The person signs in on the website and confirms; the
//! browser comes back here with a one-time code, which the shell trades (with the secret)
//! for an app token. The token is kept in the app's data folder, readable only by the user
//! (as this computer's key is), and can be revoked on the website's Account page.
//!
//! **Passes:** before hosting or joining, the shell asks the website for a pass for this
//! computer (see `pencilpilot_share::Pass`), and asks again when one is near its end.
//!
//! The website's address: `PENCILPILOT_SITE` when the app runs, else `PENCILPILOT_SITE_URL`
//! when it was built, else (debug builds) the local dev server. Its public key likewise:
//! `PENCILPILOT_PASS_KEY` (when running, then when built), else (debug builds) the
//! development key. Tokens are never logged.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pencilpilot_share::{Identity, Pass, PassInfo, SiteKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, oneshot};

/// The local dev server (`pnpm --filter @pencilpilot/web dev`), for debug builds.
const DEV_SITE: &str = "http://localhost:5173";
/// The public half of the website's development key (apps/web/src/lib/server/pass.ts).
const DEV_PASS_KEY: &str = "3b43523f310add603b4687893fafc8f094b5772e6a73d614a49b1a845c0045d0";
/// How long to wait for the person to finish in the browser.
const LINK_WAIT: Duration = Duration::from_secs(10 * 60);
/// Ask for a new pass when the one kept has less than this left.
const RENEW_BEFORE: u64 = 3600;

pub const SIGN_IN_FIRST: &str =
    "Drawing together needs a PencilPilot account: sign in with PencilPilot first.";

/// Who this app is signed in as (what the window shows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub user: String,
    pub name: String,
}

/// What's kept on disk: the token and who it's for.
#[derive(Clone, Serialize, Deserialize)]
struct Saved {
    site: String,
    token: String,
    user: String,
    name: String,
}

impl std::fmt::Debug for Saved {
    // never the token
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Saved({} as {})", self.site, self.user)
    }
}

/// Opens the system browser at a URL (replaced in tests: nothing opens a real browser).
pub type Opener = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// The account this app is signed in with, and its current pass.
pub struct Account {
    open: Opener,
    st: Mutex<St>,
    /// stops a sign-in waiting for the browser
    cancel: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

#[derive(Default)]
struct St {
    pass: Option<(Pass, PassInfo)>,
}

impl Default for Account {
    fn default() -> Self {
        Self::new(Box::new(open_browser))
    }
}

/// Where the website is.
pub fn site_url() -> Result<String, String> {
    let url = std::env::var("PENCILPILOT_SITE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| option_env!("PENCILPILOT_SITE_URL").filter(|s| !s.is_empty()).map(String::from))
        .or_else(|| cfg!(debug_assertions).then(|| DEV_SITE.to_string()))
        .ok_or("This copy of PencilPilot doesn’t know the PencilPilot website yet, so it can’t sign in.")?;
    clean_site(&url).ok_or_else(|| format!("The website’s address isn’t valid: {url}"))
}

/// An http(s) address without the trailing `/`.
fn clean_site(url: &str) -> Option<String> {
    let u = url.trim().trim_end_matches('/');
    let rest = u
        .strip_prefix("https://")
        .or_else(|| u.strip_prefix("http://"))?;
    (!rest.is_empty() && !rest.contains(['?', '#', ' '])).then(|| u.to_string())
}

/// The website's public key, to check passes with.
pub fn site_key() -> Result<SiteKey, String> {
    let hex = std::env::var("PENCILPILOT_PASS_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            option_env!("PENCILPILOT_PASS_KEY")
                .filter(|s| !s.is_empty())
                .map(String::from)
        })
        .or_else(|| cfg!(debug_assertions).then(|| DEV_PASS_KEY.to_string()))
        .ok_or("This copy of PencilPilot can’t check sign-ins yet, so it can’t draw together.")?;
    pencilpilot_share::site_key(&hex).map_err(|e| e.to_string())
}

/// Where the token is kept (`PENCILPILOT_SHARE_PROFILE` picks another, as for the key).
pub fn file(data: &Path) -> PathBuf {
    let name = match std::env::var("PENCILPILOT_SHARE_PROFILE") {
        Ok(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()) => {
            format!("account-{p}.json")
        }
        _ => "account.json".into(),
    };
    data.join("share").join(name)
}

fn load(path: &Path) -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Keep the token, readable only by the user.
fn save(path: &Path, s: &Saved) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    write_private(&tmp, &serde_json::to_vec(s).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}

/// The start of a sign-in: where to send the browser, and what to check when it's back.
#[derive(Debug, Clone)]
pub struct LinkStart {
    pub url: String,
    pub state: String,
    pub verifier: String,
    pub redirect: String,
}

fn random_text() -> String {
    data_encoding::BASE64URL_NOPAD.encode(&rand::random::<[u8; 32]>())
}

/// The code challenge for a secret (S256: base64url of its SHA-256).
pub fn challenge(verifier: &str) -> String {
    data_encoding::BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()))
}

/// The website's link page for an app listening on `port`.
pub fn link_start(site: &str, port: u16, label: &str) -> LinkStart {
    let state = random_text();
    let verifier = random_text();
    let redirect = format!("http://127.0.0.1:{port}/linked");
    let q = |s: &str| encode(s);
    let url = format!(
        "{site}/app/link?redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&label={}",
        q(&redirect),
        q(&state),
        q(&challenge(&verifier)),
        q(label)
    );
    LinkStart {
        url,
        state,
        verifier,
        redirect,
    }
}

/// What this app is called on the Account page.
fn label() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("PencilPilot on {os}")
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len()
                && b[i + 1].is_ascii_hexdigit()
                && b[i + 2].is_ascii_hexdigit() =>
            {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("00");
                out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The browser coming back: the one-time code, if the request is the sign-in's answer.
/// `Ok(None)`: some other request (the browser asking for an icon). An error: the answer
/// isn't for this sign-in (another state), or the person said no.
pub fn read_callback(request: &str, state: &str) -> Result<Option<String>, String> {
    let line = request.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Ok(None);
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/linked" {
        return Ok(None);
    }
    let mut code = None;
    let mut got_state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "code" => code = Some(decode(v)),
            "state" => got_state = Some(decode(v)),
            "error" => error = Some(decode(v)),
            _ => {}
        }
    }
    if got_state.as_deref() != Some(state) {
        return Err("That sign-in answer wasn’t for this app: try again.".into());
    }
    if error.is_some() {
        return Err("Sign-in was cancelled.".into());
    }
    code.filter(|c| !c.is_empty())
        .map(Some)
        .ok_or_else(|| "The website didn’t send a sign-in code: try again.".into())
}

/// Wait for the browser to come back with the code (other requests get a 404). The browser's
/// connection is kept to answer once the sign-in is done.
async fn wait_for_code(
    listener: &TcpListener,
    state: &str,
) -> Result<(String, TcpStream), (String, Option<TcpStream>)> {
    loop {
        let (mut stream, _) = listener.accept().await.map_err(|e| (e.to_string(), None))?;
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        let head = loop {
            let n = match tokio::time::timeout(Duration::from_secs(10), stream.read(&mut chunk))
                .await
            {
                Ok(Ok(n)) if n > 0 => n,
                _ => break None,
            };
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 16 * 1024 {
                break Some(String::from_utf8_lossy(&buf).into_owned());
            }
        };
        let Some(head) = head else { continue };
        match read_callback(&head, state) {
            Ok(Some(code)) => return Ok((code, stream)),
            Ok(None) => {
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
            }
            Err(e) => return Err((e, Some(stream))),
        }
    }
}

/// The page the browser shows at the end.
async fn answer(mut stream: TcpStream, ok: bool, text: &str) {
    let title = if ok {
        "You’re signed in"
    } else {
        "Sign-in didn’t work"
    };
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>PencilPilot</title>\
         <body style=\"font:16px system-ui;max-width:32em;margin:15vh auto;padding:0 16px\">\
         <h1 style=\"font-size:24px\">{title}</h1><p>{}</p>",
        html(text)
    );
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn http() -> Result<reqwest::Client, String> {
    // the same TLS as iroh's (ring), with the system's certificates
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("PencilPilot/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// The website's error, in its words if it gave some.
async fn site_error(r: reqwest::Response) -> String {
    let status = r.status();
    match r.json::<serde_json::Value>().await {
        Ok(v) if v["message"].is_string() => v["message"].as_str().unwrap_or_default().into(),
        _ => format!("The PencilPilot website answered {status}."),
    }
}

fn offline(_: reqwest::Error) -> String {
    "Couldn’t reach the PencilPilot website: are you online?".into()
}

#[derive(Deserialize)]
struct TokenAnswer {
    token: String,
    user: User,
}

#[derive(Deserialize)]
struct User {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct PassAnswer {
    pass: Pass,
    user: User,
}

impl Account {
    pub fn new(open: Opener) -> Self {
        Self {
            open,
            st: Mutex::new(St::default()),
            cancel: std::sync::Mutex::new(None),
        }
    }

    /// Who this app is signed in as, if anyone (for this website).
    pub async fn status(&self, data: &Path) -> Option<Status> {
        let site = site_url().ok()?;
        load(&file(data))
            .filter(|s| s.site == site)
            .map(|s| Status {
                user: s.user,
                name: s.name,
            })
    }

    /// Sign in: the browser opens at the website; done when the person has confirmed there.
    pub async fn sign_in(&self, data: &Path) -> Result<Status, String> {
        let site = site_url()?;
        site_key()?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Couldn’t get ready to sign in: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let start = link_start(&site, port, &label());
        let (tx, rx) = oneshot::channel();
        if let Some(old) = self
            .cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(tx)
        {
            let _ = old.send(());
        }
        (self.open)(&start.url)?;
        let waited = tokio::select! {
            r = wait_for_code(&listener, &start.state) => r,
            _ = rx => Err(("Sign-in was cancelled.".to_string(), None)),
            _ = tokio::time::sleep(LINK_WAIT) => Err(("Sign-in took too long: try again.".to_string(), None)),
        };
        let (code, stream) = match waited {
            Ok(x) => x,
            Err((e, stream)) => {
                if let Some(s) = stream {
                    answer(s, false, &format!("{e} You can close this tab.")).await;
                }
                return Err(e);
            }
        };
        match self.finish(data, &site, &start, &code).await {
            Ok(status) => {
                answer(
                    stream,
                    true,
                    &format!(
                        "PencilPilot is signed in as {}. You can close this tab and go back to the app.",
                        status.name
                    ),
                )
                .await;
                Ok(status)
            }
            Err(e) => {
                answer(stream, false, &format!("{e} You can close this tab.")).await;
                Err(e)
            }
        }
    }

    /// Trade the code for the app's token, and keep it.
    async fn finish(
        &self,
        data: &Path,
        site: &str,
        start: &LinkStart,
        code: &str,
    ) -> Result<Status, String> {
        let r = http()?
            .post(format!("{site}/api/app/token"))
            .json(&serde_json::json!({
                "code": code,
                "code_verifier": start.verifier,
                "redirect_uri": start.redirect,
            }))
            .send()
            .await
            .map_err(offline)?;
        if !r.status().is_success() {
            return Err(site_error(r).await);
        }
        let t: TokenAnswer = r
            .json()
            .await
            .map_err(|_| "The PencilPilot website sent something unexpected.".to_string())?;
        let saved = Saved {
            site: site.to_string(),
            token: t.token,
            user: t.user.id,
            name: t.user.name,
        };
        save(&file(data), &saved)?;
        self.st.lock().await.pass = None;
        Ok(Status {
            user: saved.user,
            name: saved.name,
        })
    }

    /// Stop a sign-in that's waiting for the browser.
    pub fn cancel(&self) {
        if let Some(tx) = self.cancel.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(());
        }
    }

    /// Sign out: the token is forgotten here and, if the website can be reached, revoked.
    pub async fn sign_out(&self, data: &Path) {
        self.st.lock().await.pass = None;
        let path = file(data);
        if let Some(s) = load(&path) {
            let _ = std::fs::remove_file(&path);
            if let Ok(c) = http() {
                let _ = c
                    .post(format!("{}/api/app/unlink", s.site))
                    .bearer_auth(&s.token)
                    .send()
                    .await;
            }
        }
    }

    /// A pass for this computer (`id`), and the website's key to check others' with: the
    /// one kept if it has a while left, else a new one from the website.
    pub async fn pass(&self, data: &Path, id: &Identity) -> Result<(Pass, SiteKey), String> {
        let key = site_key()?;
        let site = site_url()?;
        let path = file(data);
        let saved = load(&path)
            .filter(|s| s.site == site)
            .ok_or(SIGN_IN_FIRST)?;
        let mut st = self.st.lock().await;
        let now = pencilpilot_share::now();
        if let Some((p, info)) = &st.pass
            && info.endpoint == id.id()
            && info.user == saved.user
            && info.expires > now + RENEW_BEFORE
        {
            return Ok((p.clone(), key));
        }
        let r = http()?
            .post(format!("{site}/api/app/pass"))
            .bearer_auth(&saved.token)
            .json(&serde_json::json!({ "endpoint": id.id() }))
            .send()
            .await
            .map_err(offline)?;
        if r.status() == reqwest::StatusCode::UNAUTHORIZED {
            // unlinked on the website
            let _ = std::fs::remove_file(&path);
            st.pass = None;
            return Err(
                "This app was unlinked from your PencilPilot account: sign in again.".into(),
            );
        }
        if !r.status().is_success() {
            return Err(site_error(r).await);
        }
        let a: PassAnswer = r
            .json()
            .await
            .map_err(|_| "The PencilPilot website sent something unexpected.".to_string())?;
        let info = a.pass.check(&key, &id.id(), now).map_err(|_| {
            "The website’s sign-in pass doesn’t match this app: is it the right website?"
                .to_string()
        })?;
        if a.user.name != saved.name || a.user.id != saved.user {
            let _ = save(
                &path,
                &Saved {
                    name: a.user.name,
                    user: a.user.id,
                    ..saved
                },
            );
        }
        st.pass = Some((a.pass.clone(), info));
        Ok((a.pass, key))
    }
}

/// The system's browser (never in tests).
fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn’t open the browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pp-account-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_link_page_gets_a_loopback_address_and_a_challenge() {
        let s = link_start("https://pencilpilot.example", 53111, "PencilPilot on Linux");
        assert_eq!(s.redirect, "http://127.0.0.1:53111/linked");
        assert!(s.url.starts_with("https://pencilpilot.example/app/link?"));
        assert!(
            s.url
                .contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A53111%2Flinked")
        );
        assert!(s.url.contains(&format!("state={}", s.state)));
        assert!(
            s.url
                .contains(&format!("code_challenge={}", challenge(&s.verifier)))
        );
        assert!(s.url.contains("code_challenge_method=S256"));
        assert!(s.url.contains("label=PencilPilot%20on%20Linux"));
        // the secret itself never goes to the browser
        assert!(!s.url.contains(&s.verifier));
        assert_eq!(s.state.len(), 43);
        assert_ne!(
            s.state,
            link_start("x", 1, "").state,
            "a new state each time"
        );
        // RFC 7636's example
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn only_this_sign_in_s_answer_is_taken() {
        let ok = "GET /linked?code=abc%2Bd&state=s1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(read_callback(ok, "s1"), Ok(Some("abc+d".into())));
        // someone else's sign-in, or a page making the browser call here
        let e = read_callback(ok, "s2").unwrap_err();
        assert!(e.contains("wasn’t for this app"), "{e}");
        assert!(read_callback("GET /linked?code=abc HTTP/1.1\r\n\r\n", "s1").is_err());
        // said no on the website
        let no = "GET /linked?error=access_denied&state=s1 HTTP/1.1\r\n\r\n";
        assert_eq!(
            read_callback(no, "s1"),
            Err("Sign-in was cancelled.".into())
        );
        // other requests are ignored
        assert_eq!(
            read_callback("GET /favicon.ico HTTP/1.1\r\n\r\n", "s1"),
            Ok(None)
        );
        assert_eq!(
            read_callback("POST /linked HTTP/1.1\r\n\r\n", "s1"),
            Ok(None)
        );
    }

    #[test]
    fn the_site_address_is_tidied() {
        assert_eq!(
            clean_site(" https://pencilpilot.example/ "),
            Some("https://pencilpilot.example".into())
        );
        assert_eq!(
            clean_site("http://localhost:5173"),
            Some("http://localhost:5173".into())
        );
        assert_eq!(clean_site("ftp://x"), None);
        assert_eq!(clean_site("https://"), None);
        // debug builds know the dev server and the development key
        assert!(site_key().is_ok());
    }

    #[test]
    fn the_token_is_kept_for_the_user_only() {
        let dir = temp("keep");
        let path = file(&dir);
        let s = Saved {
            site: "http://localhost:5173".into(),
            token: "ppa_secret".into(),
            user: "u_ana".into(),
            name: "Ana".into(),
        };
        save(&path, &s).unwrap();
        save(&path, &s).unwrap(); // again, over the old one
        let back = load(&path).unwrap();
        assert_eq!(
            (back.token.as_str(), back.name.as_str()),
            ("ppa_secret", "Ana")
        );
        assert!(!format!("{back:?}").contains("ppa_secret"), "never logged");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A tiny stand-in for the website: answers the token exchange.
    async fn fake_site(token_status: u16) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let mut got = Vec::new();
            loop {
                let n = s.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&got);
                if let Some(i) = text.find("\r\n\r\n") {
                    let len: usize = text
                        .lines()
                        .find_map(|l| {
                            l.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap_or(0);
                    if got.len() >= i + 4 + len {
                        break;
                    }
                }
            }
            let body = if token_status == 200 {
                r#"{"token":"ppa_new","user":{"id":"u_ana","name":"Ana"}}"#
            } else {
                r#"{"error":"invalid_grant","message":"That sign-in didn't work."}"#
            };
            let head = format!(
                "HTTP/1.1 {token_status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            s.write_all(head.as_bytes()).await.unwrap();
            s.write_all(body.as_bytes()).await.unwrap();
            vec![String::from_utf8_lossy(&got).into_owned()]
        });
        (url, h)
    }

    /// The browser, played by the test: follows the link page straight back to the app.
    fn fake_browser(state_override: Option<&'static str>) -> Opener {
        Box::new(move |url: &str| {
            let q = url.split_once('?').unwrap().1;
            let get = |k: &str| {
                q.split('&')
                    .find_map(|p| p.strip_prefix(&format!("{k}=")))
                    .map(decode)
                    .unwrap()
            };
            let redirect = get("redirect_uri");
            let state = state_override.map(String::from).unwrap_or(get("state"));
            let addr = redirect
                .strip_prefix("http://")
                .unwrap()
                .split('/')
                .next()
                .unwrap()
                .to_string();
            tokio::spawn(async move {
                // an icon request first, as browsers do
                for path in [
                    "/favicon.ico".to_string(),
                    format!("/linked?code=one-time&state={state}"),
                ] {
                    let mut s = TcpStream::connect(&addr).await.unwrap();
                    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
                        .await
                        .unwrap();
                    let mut page = String::new();
                    let _ = s.read_to_string(&mut page).await;
                }
            });
            Ok(())
        })
    }

    #[tokio::test]
    async fn signing_in_keeps_the_token_and_signing_out_forgets_it() {
        let (site, seen) = fake_site(200).await;
        // SAFETY: tests that read these run in this one test
        unsafe { std::env::set_var("PENCILPILOT_SITE", &site) };
        let dir = temp("flow");
        let account = Account::new(fake_browser(None));
        assert_eq!(account.status(&dir).await, None);
        let me = account.sign_in(&dir).await.unwrap();
        assert_eq!(me.name, "Ana");
        assert_eq!(account.status(&dir).await, Some(me));
        let asked = seen.await.unwrap().join("");
        assert!(asked.starts_with("POST /api/app/token"), "{asked}");
        assert!(asked.contains("\"code\":\"one-time\""), "{asked}");
        assert!(asked.contains("code_verifier"), "{asked}");
        assert_eq!(load(&file(&dir)).unwrap().token, "ppa_new");

        // the wrong state never reaches the website
        let account2 = Account::new(fake_browser(Some("forged-state")));
        let e = account2.sign_in(&temp("forged")).await.unwrap_err();
        assert!(e.contains("wasn’t for this app"), "{e}");

        // signing out (the website is gone by now: forgotten here all the same)
        account.sign_out(&dir).await;
        assert_eq!(account.status(&dir).await, None);
        let e = account.pass(&dir, &Identity::generate()).await.unwrap_err();
        assert_eq!(e, SIGN_IN_FIRST);
        let _ = std::fs::remove_dir_all(dir);
    }
}
