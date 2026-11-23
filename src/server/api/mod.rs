//! The dedicated JSON API (SSR only).
//!
//! Plain Axum routes under `/api/*` returning JSON. This now covers only the
//! endpoints that must stay REST: the embeddable Squarespace chat widget
//! (cross-origin, hence [`cors_layer`]) plus health/version/docs. The app's own
//! authenticated operations are Leptos server functions in
//! [`crate::server_fns`], served under `/api/rpc/*`.
//!
//! Each feature area lives in its own submodule exposing a `routes()` function;
//! [`router`] merges them into one router.

pub mod chat;
pub mod contact_export;
pub mod cors;
pub mod docs;
pub mod health;
pub mod message_transcripts;
pub mod version;
pub mod volunteer_agreement;

pub use cors::cors_layer;

use axum::Router;

/// Build the combined `/api/*` router. Generic over state so it can be merged
/// into the Leptos router (which carries `LeptosOptions` state); handlers
/// ignore it.
pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .merge(health::routes())
        .merge(version::routes())
        .merge(chat::routes())
        .merge(docs::routes())
}
