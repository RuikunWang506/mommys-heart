//! Database access layer (SSR only): connection pool, migrations, and the
//! repositories that persist application data in PostgreSQL.
//!
//! The pool lives in a process-wide [`OnceLock`] because the SSR branch of
//! [`crate::api_client`] calls the service/repository layer directly (it has no
//! Axum state handle to thread a pool through).

pub mod admin_requests;
pub mod audit;
pub mod bulk_properties;
pub mod capabilities;
pub mod case_contacts;
pub mod case_documents;
pub mod case_note_documents;
pub mod case_notes;
pub mod case_plan;
pub mod case_properties;
pub mod cases;
pub mod channel_notifications;
pub mod channels;
pub mod client_agreements;
pub mod clients;
pub mod contact_directory;
pub mod contact_mail;
pub mod contact_properties;
pub mod contact_rules;
pub mod contacts;
pub mod crm_import;
pub mod deactivations;
pub mod email_failures;
pub mod funding;
pub mod grants;
pub mod ids;
pub mod messages;
pub mod mfa;
pub mod organization_properties;
pub mod organizations;
pub mod password_reset;
pub mod pending_registrations;
pub mod property_filters;
pub mod reports;
pub mod seed;
pub mod sessions;
pub mod settings;
pub mod terms_acceptances;
pub mod throttle;
pub mod trusted_devices;
pub mod users;
pub mod volunteer_applicants;
pub mod volunteer_hours;
pub mod volunteers;

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::OnceLock;

static POOL: OnceLock<PgPool> = OnceLock::new();

/// The initialized connection pool. Panics if [`init`] has not run yet.
pub fn pool() -> &'static PgPool {
    POOL.get()
        .expect("database pool not initialized; call server::db::init() first")
}

/// Where development connects when `DATABASE_URL` says nothing.
///
/// The shared database Kingdom raises from `.kingdom/services.toml`, which an
/// isolated plan reaches on its own loopback at the stock port. The credentials
/// are the image's defaults because that is all a service manifest can ask for:
/// it declares an image and a port, and Kingdom supplies `POSTGRES_PASSWORD` to
/// make `postgres:16` boot at all.
const DEV_DATABASE_URL: &str = "postgres://postgres:postgres@localhost:5432/postgres";

/// Connect to `DATABASE_URL`, run migrations, and seed the database if it is
/// empty. Safe to call once at startup.
///
/// Outside production the URL defaults to [`DEV_DATABASE_URL`], so an agent
/// runs the app with nothing configured at all. Production keeps failing fast
/// on a missing variable: defaulting there would silently point a deployment at
/// a database that is not its own.
pub async fn init() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => url,
        _ if crate::server::config::is_production() => {
            return Err("DATABASE_URL is not set (see .env / .env.example)".into())
        }
        _ => {
            tracing::info!(
                "DATABASE_URL unset; using the shared dev database at {DEV_DATABASE_URL}"
            );
            DEV_DATABASE_URL.to_string()
        }
    };

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    POOL.set(pool)
        .map_err(|_| "database pool already initialized")?;

    // A fresh volume comes up with a schema and no users, which would leave no
    // way to log in. Seeding is skipped the moment any user exists, so this is
    // a one-off cost on a new database rather than something every boot pays.
    if !crate::server::config::is_production() {
        seed::seed_if_empty().await?;
    }

    Ok(())
}

/// Server-side "now" as `YYYY-MM-DD HH:MM` in local time, matching the display
/// format the browser produces via `state::now_stamp`.
pub fn now_stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M").to_string()
}
