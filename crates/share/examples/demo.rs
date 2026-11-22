//! Try drawing together without a second person:
//!
//!     cargo run -p pencilpilot-share --example demo            # host: prints a code
//!     cargo run -p pencilpilot-share --example demo -- pp1-…   # join that code
//!
//! It moves a cursor in a circle around (500, 400) and prints what others do. Paste the
//! printed code into the app's **Draw together → Join** to see the cursor there.
//! `PENCILPILOT_RELAY=off` keeps it on this network.

use std::time::Duration;

use pencilpilot_share::{Event, Identity, Invite, Relays, Session};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let relays = match std::env::var("PENCILPILOT_RELAY").as_deref() {
        Ok("off") => Relays::Off,
        _ => Relays::Iroh,
    };
    let id = Identity::generate();
    let mut s = match std::env::args().nth(1) {
        Some(code) => Session::join(&id, &Invite::decode(&code)?, "Demo", relays).await?,
        None => {
            let s = Session::host(&id, "Demo", relays).await?;
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
    loop {
        tokio::select! {
            _ = tick.tick() => {
                a += 0.08;
                s.presence(json!({ "x": (500.0 + 200.0 * a.cos()).round(), "y": (400.0 + 200.0 * a.sin()).round() }));
            }
            e = s.next_event() => match e {
                Some(Event::Presence { .. }) => {}
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
