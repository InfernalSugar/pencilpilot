//! Try drawing together without a second person:
//!
//!     cargo run -p pencilpilot-share --example demo            # host: prints a code
//!     cargo run -p pencilpilot-share --example demo -- pp1-…   # join that code
//!
//! It moves a cursor in a circle around (500, 400), hands an app that joins an empty project
//! (so the app gets going), and prints what the others do, briefly (an edit's kind, layer
//! and number of tiles, not its pixels). Paste the printed code into the app's
//! **Draw together → Join**. `PENCILPILOT_RELAY=off` keeps it on this network.
//!
//! Drawing together needs a sign-in pass: the demo signs its own with the website's
//! development key (the one `pnpm --filter @pencilpilot/web dev` uses when
//! `PASS_SIGNING_KEY` isn't set), which a debug build of the app trusts. Give another key
//! (64 hex characters) in `PASS_SIGNING_KEY` to match a site that has its own.

use std::time::Duration;

use pencilpilot_share::{Event, Identity, Invite, Pass, PassInfo, Relays, SecretKey, Session};
use serde_json::{Value, json};

/// The website's development signing key (public knowledge: development only).
const DEV_SEED: &str = "62e1111add10abf5114b66e2caee869c95d97cf13b95d154524efcfee558cbf6";

/// An edit, in a line.
fn brief(body: &Value) -> String {
    let k = body["k"].as_str().unwrap_or("?");
    let lid = body["lid"].as_str().unwrap_or("");
    match k {
        "tiles" => format!(
            "tiles on {lid} (drawing {}): {} set, {} cleared",
            body["did"].as_str().unwrap_or("?"),
            body["set"].as_array().map_or(0, |a| a.len()),
            body["clear"].as_array().map_or(0, |a| a.len())
        ),
        "timeline" => format!(
            "timeline: {} scene(s), {} cut(s) in the first",
            body["tl"]["scenes"].as_array().map_or(0, |a| a.len()),
            body["tl"]["scenes"][0]["cuts"]
                .as_array()
                .map_or(0, |a| a.len())
        ),
        "layer" | "rename" => format!("{k} {lid} \"{}\"", body["name"].as_str().unwrap_or("")),
        _ => format!("{k} {lid} {}", body["to"].as_str().unwrap_or("")),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // RUST_LOG=iroh=debug (etc.): the connection's own logs
    if std::env::var_os("RUST_LOG").is_some() {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .init();
    }
    let relays = match std::env::var("PENCILPILOT_RELAY").as_deref() {
        Ok("off") => Relays::Off,
        _ => Relays::Iroh,
    };
    let id = Identity::generate();
    let seed = std::env::var("PASS_SIGNING_KEY").unwrap_or_else(|_| DEV_SEED.into());
    let seed: [u8; 32] = data_encoding::HEXLOWER_PERMISSIVE
        .decode(seed.trim().as_bytes())?
        .try_into()
        .map_err(|_| "PASS_SIGNING_KEY: 64 hex characters")?;
    let site = SecretKey::from_bytes(&seed);
    let now = pencilpilot_share::now();
    let pass = Pass::sign(
        &site,
        &PassInfo {
            user: "u_demo".into(),
            name: "Demo".into(),
            endpoint: id.id(),
            issued: now,
            expires: now + 24 * 3600,
        },
    );
    let site = site.public();
    let mut s = match std::env::args().nth(1) {
        Some(code) => Session::join(&id, &Invite::decode(&code)?, &pass, &site, relays).await?,
        None => {
            let s = Session::host(&id, &pass, &site, relays).await?;
            println!("Invite code:\n\n{}\n", s.invite().encode());
            s
        }
    };
    println!(
        "Connected as {} ({}); Ctrl+C to stop.",
        s.me().name,
        s.me().id
    );
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let mut a: f64 = 0.0;
    let mut seq = 0;
    let mut cursors = 0u64;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                a += 0.08;
                s.presence(json!({ "x": (500.0 + 200.0 * a.cos()).round(), "y": (400.0 + 200.0 * a.sin()).round() }));
            }
            e = s.next_event() => match e {
                Some(Event::Presence { from, body }) => {
                    if let Some(live) = body.get("live") {
                        println!("live stroke from {}: {}", &from[..8], live["type"].as_str().unwrap_or("?"));
                    } else {
                        cursors += 1;
                        if cursors % 40 == 1 {
                            println!("cursor from {} at ({}, {})", &from[..8], body["x"], body["y"]);
                        }
                    }
                }
                Some(Event::Direct { from, body }) => {
                    println!("direct from {}: {}", &from[..8], body["k"]);
                    // an app joining asks for the project: an empty one, so it gets going
                    if body["k"] == "snap.req" {
                        s.direct(&from, json!({ "k": "snap.begin", "seq": seq, "layers": [], "asks": [], "tl": null }))?;
                        s.direct(&from, json!({ "k": "snap.end" }))?;
                    }
                }
                Some(Event::Edit { seq: n, from, body }) => {
                    seq = n;
                    println!("edit {n} from {}: {}", &from[..8], brief(&body));
                }
                Some(Event::Closed { reason }) => { println!("{reason}"); break; }
                Some(e) => println!("{e:?}"),
                None => break,
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    s.close().await;
    Ok(())
}
