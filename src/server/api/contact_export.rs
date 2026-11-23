//! Authenticated Excel export of the contact directory.
//!
//! `GET /api/contacts/export.xlsx` takes the same query parameters the Contacts
//! page keeps in its URL (`q`, `type`, `org`, `cat`, `props`, `archived`), so
//! the file holds exactly the contacts on screen, across every page.

use std::collections::HashMap;

use axum::{
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::server::auth::AuthUser;
use crate::server::db::contact_directory as directory;
use crate::server::db::{audit, pool};
use crate::server::permissions::require_information_management_access;
use crate::server::xlsx::{self, Column};
use crate::server_fns::contact_directory::Contact;
use crate::server_fns::contacts::ContactType;
use crate::server_fns::property_filters;

/// The most contacts one export holds, so a single request cannot run without
/// bound. The directory is far smaller than this today.
const MAX_EXPORT_ROWS: usize = 10_000;
/// Page size used to walk the directory; the directory query caps pages at 100.
const PAGE: i64 = 100;

pub fn install<S>(router: Router<S>) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route("/api/contacts/export.xlsx", get(download))
}

async fn download(
    AuthUser(user): AuthUser,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    // Same gate as the Contacts page search.
    if require_information_management_access(&user).is_err()
        || !user.role.has_volunteer_privileges()
    {
        return (
            StatusCode::FORBIDDEN,
            "You do not have access to the contact directory.",
        )
            .into_response();
    }

    let get = |key: &str| {
        params
            .get(key)
            .map(|v| v.trim().to_string())
            .unwrap_or_default()
    };
    let query = get("q");
    let contact_type = ContactType::from_slug(&get("type"));
    let organization_id = get("org");
    let mut category_ids: Vec<String> = get("cat")
        .split(',')
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    category_ids.sort();
    category_ids.dedup();
    let include_archived = get("archived") == "1";
    let filters = property_filters::clean(property_filters::decode(&get("props")));
    let include_account_projection = user.role.has_operations_admin_permissions();

    let mut contacts: Vec<Contact> = Vec::new();
    loop {
        let page = match directory::search_page(
            &query,
            &category_ids,
            contact_type,
            &organization_id,
            include_archived,
            &filters,
            contacts.len() as i64,
            PAGE,
            include_account_projection,
        )
        .await
        {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!("contact export query failed: {error}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not export contacts.",
                )
                    .into_response();
            }
        };
        let fetched = page.items.len();
        contacts.extend(page.items);
        if fetched == 0 || contacts.len() as i64 >= page.total || contacts.len() >= MAX_EXPORT_ROWS
        {
            break;
        }
    }
    contacts.truncate(MAX_EXPORT_ROWS);

    // Exporting copies personal details out of the CRM, so leave a trace of who
    // did it and how much, on the exporter's own change log.
    if let Err(error) = audit::record(
        pool(),
        audit::Entity::User,
        &user.id,
        &user.full_name(),
        "contact directory export",
        "",
        &format!("exported {} contacts to Excel", contacts.len()),
    )
    .await
    {
        tracing::warn!("contact export audit failed: {error}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not export contacts.",
        )
            .into_response();
    }

    match render(&contacts) {
        Ok(bytes) => {
            let filename = format!("Contacts {}.xlsx", chrono::Local::now().format("%Y-%m-%d"));
            (
                [
                    (
                        header::CONTENT_TYPE,
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                            .to_string(),
                    ),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{filename}\""),
                    ),
                    (header::CACHE_CONTROL, "no-store".to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(error) => {
            tracing::warn!("contact export render failed: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not export contacts.",
            )
                .into_response()
        }
    }
}

fn render(contacts: &[Contact]) -> Result<Vec<u8>, String> {
    let columns = [
        Column {
            header: "Name",
            width: 28.0,
        },
        Column {
            header: "Organization",
            width: 28.0,
        },
        Column {
            header: "Job title",
            width: 22.0,
        },
        Column {
            header: "Email",
            width: 30.0,
        },
        Column {
            header: "Phone",
            width: 18.0,
        },
        Column {
            header: "Address",
            width: 36.0,
        },
        Column {
            header: "Website",
            width: 26.0,
        },
        Column {
            header: "Types",
            width: 24.0,
        },
        Column {
            header: "Categories",
            width: 36.0,
        },
        Column {
            header: "Has account",
            width: 12.0,
        },
        Column {
            header: "Archived",
            width: 10.0,
        },
        Column {
            header: "Last updated",
            width: 18.0,
        },
    ];
    let yes_no = |value: bool| if value { "Yes" } else { "No" }.to_string();
    let rows: Vec<Vec<String>> = contacts
        .iter()
        .map(|c| {
            vec![
                c.full_name.clone(),
                c.organization.clone(),
                c.title.clone(),
                c.email.clone(),
                c.phone.clone(),
                c.address.clone(),
                c.website.clone(),
                c.types
                    .iter()
                    .map(|t| t.label())
                    .collect::<Vec<_>>()
                    .join(", "),
                c.categories
                    .iter()
                    .map(|category| category.label())
                    .collect::<Vec<_>>()
                    .join("; "),
                yes_no(c.has_account),
                yes_no(c.archived),
                c.updated_at.clone(),
            ]
        })
        .collect();
    xlsx::workbook("Contacts", &columns, &rows)
}
