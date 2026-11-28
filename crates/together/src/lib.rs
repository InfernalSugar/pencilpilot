//! Drawing together, the drawing side (ADR-0013): shared layers, who may draw on each,
//! and their pixels kept the same in every app. A port of the studio's
//! `apps/studio/src/lib/together.ts`, wire-compatible with it, so the studio and the
//! native app share one live session.
//!
//! - **One drawer per layer at a time.** A layer has an owner (who made it) and the people
//!   allowed to draw on it ("writers"). Only their changes to it count; everyone else sees
//!   it locked. Asking for access: `ask` → the owner's `grant` (let them draw, or hand it
//!   over), `deny`, or later `revoke`.
//! - **Groups** are shared the same way: a group has an owner and people who may change it
//!   (its name, eye and opacity, and which layers are in it).
//! - **Merging and grouping others' layers** asks first: what's missing is asked for (say
//!   why: to merge, to group, to delete), and the change happens once it's granted. Merging
//!   a layer away needs it handed over; merging into a layer, or grouping it, needs leave to
//!   draw on it.
//! - **Pixels travel as tiles** (64×64, as in `.ppstudio`): after each change, the drawer's
//!   app sends the tiles of their layers that changed (compressed); the others put them
//!   in. Fills, transforms and imports travel the same way as strokes.
//! - **Layers stack in the order they were made**, which every app sees the same (edits are
//!   numbered by the session), so no app ever reorders anything.
//! - **Joining:** the new member asks the host for the project (a snapshot of the layers,
//!   their access and tiles), then applies the edits numbered after it.
//! - **The edits are a closed list** ([`Edit`]): anything else, AI-made content included,
//!   is dropped on arrival ([`parse_edit`]). AI is solo only.
//!
//! No networking here, and no canvas: [`Together`] takes the canvas as a [`CanvasPort`]
//! on each call and queues what to send ([`Out`]) for the app to pass to
//! `pencilpilot_share::Session`.
//!
//! ```text
//! session event              →  Together                      →  the app
//! Event::Edit {seq,from,body}   on_edit(canvas, seq, from, &body)   take_out() → Session::edit / direct
//! Event::Direct {from,body}     on_direct(canvas, from, &body)      take_notices() → notes, redraws
//! (chat first: chat::chat_text)
//! the canvas changed            local_changed(canvas)
//! the timeline changed          timeline_changed(canvas)
//! ```

pub mod chat;
pub mod edit;
pub mod js;
pub mod lines;
pub mod saved;
pub mod shared;
pub mod tiles;
pub mod timeline;
mod together;

pub use edit::{Edit, LayerStyle, Look, StrokePosition, Why, parse_edit, same_look, same_style};
pub use lines::LineStroke;
pub use saved::{SavedMember, SavedShare};
pub use shared::{Ask, Shared, SharedGroup, SharedLayer};
pub use tiles::{TILE, TILE_BYTES, TileMap, decode_tile, diff_tiles, encode_tile, pack, unpack};
pub use timeline::Timeline;
pub use together::{
    CanvasPort, GroupChange, GroupSettings, LayerPatch, Notice, Out, PortGroup, PortLayer, Strokes,
    Together,
};
