//! Server-side (SSR-only) code: the dedicated JSON API, authorization layer,
//! RAG chatbot pipeline, and CAPTCHA verification. The app's request handlers
//! themselves live in [`crate::server_fns`] as Leptos server functions.

pub mod api;
pub mod auth;
pub mod captcha;
pub mod case_note_records;
pub mod config;
pub mod contact_mail;
pub mod crm_import;
pub mod db;
pub mod docs;
pub mod email;
pub mod notifications;
pub mod permissions;
pub mod rag;
pub mod reports;
pub mod security;
pub mod service;
pub mod sharepoint;
pub mod sheets;
pub mod telemetry;
pub mod volunteer_agreement_pdf;
pub mod xlsx;
