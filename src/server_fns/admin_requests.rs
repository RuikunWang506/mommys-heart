//! Approval requests filed by operations admins and decided by site admins.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ssr")]
use crate::server_fns::capabilities::validate_capabilities;
use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::pagination::Page;
use crate::server_fns::users::AccountRole;

#[cfg(feature = "ssr")]
const MAX_HISTORY_PAGE: i64 = 1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminRequestKind {
    Role,
    CaseCapabilities,
    InformationAccess,
}

impl AdminRequestKind {
    pub const ALL: [Self; 3] = [Self::Role, Self::CaseCapabilities, Self::InformationAccess];

    pub fn label(self) -> &'static str {
        match self {
            Self::Role => "Role change",
            Self::CaseCapabilities => "Case permissions",
            Self::InformationAccess => "Information access",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Role => "role",
            Self::CaseCapabilities => "case_capabilities",
            Self::InformationAccess => "information_access",
        }
    }

    pub fn from_slug(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.slug() == value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminRequestStatus {
    Pending,
    Approved,
    Denied,
}

impl AdminRequestStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "Pending",
            Self::Approved => "Approved",
            Self::Denied => "Denied",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdminRequest {
    pub id: String,
    pub kind: AdminRequestKind,
    pub status: AdminRequestStatus,
    pub requested_by_id: String,
    pub requested_by_name: String,
    pub target_user_id: String,
    pub target_user_name: String,
    pub case_id: Option<String>,
    pub case_name: Option<String>,
    pub current_role: Option<AccountRole>,
    pub requested_role: Option<AccountRole>,
    /// `None` means the user currently has no assignment to this case.
    pub current_capabilities: Option<Vec<CaseCapability>>,
    /// `None` means the request asks to remove the assignment.
    pub requested_capabilities: Option<Vec<CaseCapability>>,
    pub current_information_access: Option<bool>,
    pub requested_information_access: Option<bool>,
    pub request_note: String,
    pub created_at: String,
    pub decided_by_name: Option<String>,
    pub decision_note: String,
    pub decided_at: Option<String>,
}

impl AdminRequest {
    pub fn change_summary(&self) -> String {
        match self.kind {
            AdminRequestKind::Role => format!(
                "{} to {}",
                self.current_role
                    .map(AccountRole::label)
                    .unwrap_or("Unknown"),
                self.requested_role
                    .map(AccountRole::label)
                    .unwrap_or("Unknown"),
            ),
            AdminRequestKind::CaseCapabilities => format!(
                "{} to {}",
                capability_summary(self.current_capabilities.as_deref()),
                capability_summary(self.requested_capabilities.as_deref()),
            ),
            AdminRequestKind::InformationAccess => format!(
                "{} to {}",
                information_access_summary(self.current_information_access),
                information_access_summary(self.requested_information_access),
            ),
        }
    }
}

fn capability_summary(capabilities: Option<&[CaseCapability]>) -> String {
    match capabilities {
        None => "Not assigned".to_string(),
        Some(capabilities) => capabilities
            .iter()
            .map(|capability| capability.label())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn information_access_summary(access: Option<bool>) -> &'static str {
    match access {
        Some(true) => "Granted",
        Some(false) => "Denied",
        None => "Unknown",
    }
}

/// List pending requests visible to the caller. Site admins see the approval
/// queue; operations admins see only requests they filed.
#[server(prefix = "/api")]
pub async fn list_active_admin_requests() -> Result<Vec<AdminRequest>, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    admin_requests::list_active(&actor.id, actor.role.is_site_admin())
        .await
        .map_err(ServerFnError::new)
}

/// List pending requests of the requested kind visible to the caller. Site
/// admins see the approval queue; operations admins see only requests they
/// filed.
#[server(prefix = "/api")]
pub async fn list_active_admin_requests_by_kind(
    kind: AdminRequestKind,
) -> Result<Vec<AdminRequest>, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    admin_requests::list_active_by_kind(&actor.id, actor.role.is_site_admin(), kind)
        .await
        .map_err(ServerFnError::new)
}

/// List resolved requests visible to the caller using the shared pagination
/// envelope. Site admins see all history; operations admins see their own.
#[server(prefix = "/api")]
pub async fn list_admin_request_history(
    offset: i64,
    limit: i64,
) -> Result<Page<AdminRequest>, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    admin_requests::history_page(
        &actor.id,
        actor.role.is_site_admin(),
        offset.max(0),
        limit.clamp(1, MAX_HISTORY_PAGE),
    )
    .await
    .map_err(ServerFnError::new)
}

/// List resolved requests of the requested kind visible to the caller using
/// the shared pagination envelope. Site admins see all history; operations
/// admins see their own.
#[server(prefix = "/api")]
pub async fn list_admin_request_history_by_kind(
    kind: AdminRequestKind,
    offset: i64,
    limit: i64,
) -> Result<Page<AdminRequest>, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    admin_requests::history_page_by_kind(
        &actor.id,
        actor.role.is_site_admin(),
        kind,
        offset.max(0),
        limit.clamp(1, MAX_HISTORY_PAGE),
    )
    .await
    .map_err(ServerFnError::new)
}

/// Number of requests currently waiting for site-admin review.
#[server(prefix = "/api")]
pub async fn pending_admin_request_count() -> Result<i64, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    admin_requests::pending_count()
        .await
        .map_err(ServerFnError::new)
}

/// Number of requests of one kind currently waiting for site-admin review.
#[server(prefix = "/api")]
pub async fn pending_admin_request_count_by_kind(
    kind: AdminRequestKind,
) -> Result<i64, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    admin_requests::pending_count_by_kind(kind)
        .await
        .map_err(ServerFnError::new)
}

/// Request a change to another user's global role.
#[server(prefix = "/api")]
pub async fn request_user_role_change(
    target_user_id: String,
    requested_role: AccountRole,
    note: String,
) -> Result<AdminRequest, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    let target_user_id = target_user_id.trim();
    if target_user_id.is_empty() || target_user_id == actor.id {
        return Err(ServerFnError::new(
            "Choose another user for this role-change request.",
        ));
    }
    let note = validate_note(note)?;
    let request = admin_requests::create_role(&actor.id, target_user_id, requested_role, &note)
        .await
        .map_err(|error| ServerFnError::new(error.to_string()))?;
    crate::server::notifications::notify_admin_request_filed(request.clone());
    Ok(request)
}

/// Request multiple case-permission changes atomically. Either every request is
/// created or none are, so a multi-case draft cannot be partially submitted.
#[server(prefix = "/api", input = leptos::server_fn::codec::Json)]
pub async fn request_user_case_capabilities(
    target_user_id: String,
    changes: Vec<(String, Option<Vec<CaseCapability>>)>,
    note: String,
) -> Result<Vec<AdminRequest>, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    let target_user_id = target_user_id.trim().to_string();
    if target_user_id.is_empty() || target_user_id == actor.id {
        return Err(ServerFnError::new(
            "Invalid user choice for case id permissions request",
        ));
    }
    if changes.is_empty() {
        return Err(ServerFnError::new(
            "Choose at least one case-permission change.",
        ));
    }

    let mut validated_changes = Vec::with_capacity(changes.len());
    for (case_id, requested_capabilities) in changes {
        let case_id = case_id.trim().to_string();
        if case_id.is_empty() {
            return Err(ServerFnError::new("Choose a case for this request."));
        }
        if let Some(capabilities) = &requested_capabilities {
            if capabilities.is_empty() {
                return Err(ServerFnError::new(
                    "Choose at least one permission, or request removal.",
                ));
            }
            validate_capabilities(capabilities).map_err(ServerFnError::new)?;
        }
        validated_changes.push((case_id, requested_capabilities));
    }

    let note = validate_note(note)?;
    let requests = admin_requests::create_case_capabilities(
        &actor.id,
        &target_user_id,
        &validated_changes,
        &note,
    )
    .await
    .map_err(|error| ServerFnError::new(error.to_string()))?;
    for request in &requests {
        crate::server::notifications::notify_admin_request_filed(request.clone());
    }
    Ok(requests)
}

/// Request the existing information-area grant for an eligible user.
#[server(prefix = "/api")]
pub async fn request_user_information_access(
    target_user_id: String,
    note: String,
) -> Result<AdminRequest, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    let target_user_id = target_user_id.trim();
    if target_user_id.is_empty() {
        return Err(ServerFnError::new(
            "Choose a user for this information-access request.",
        ));
    }
    let note = validate_note(note)?;
    let request = admin_requests::create_information_access(&actor.id, target_user_id, &note)
        .await
        .map_err(|error| ServerFnError::new(error.to_string()))?;
    crate::server::notifications::notify_admin_request_filed(request.clone());
    Ok(request)
}

/// Approve or deny one pending request. Site-admin only.
#[server(prefix = "/api")]
pub async fn decide_admin_request(
    request_id: String,
    approve: bool,
    note: String,
) -> Result<AdminRequest, ServerFnError> {
    use crate::server::db::admin_requests;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    let note = validate_note(note)?;
    let outcome = admin_requests::decide(
        request_id.trim(),
        approve,
        &actor.id,
        &actor.full_name(),
        &note,
    )
    .await
    .map_err(|error| ServerFnError::new(error.to_string()))?;
    if approve
        && outcome.request.kind == AdminRequestKind::CaseCapabilities
        && outcome.was_unassigned
        && outcome
            .request
            .requested_capabilities
            .as_deref()
            .is_some_and(|caps| caps.contains(&CaseCapability::ViewCase))
    {
        if let Some(case_id) = &outcome.request.case_id {
            crate::server::notifications::notify_assignment(
                outcome.request.target_user_id.clone(),
                actor.full_name(),
                case_id.clone(),
            );
        }
    }
    // An approved capability change rewrites `case_assignments` directly, so the
    // library's sharing is reconciled here as well as at the other write sites.
    if approve && outcome.request.kind == AdminRequestKind::CaseCapabilities {
        if let Some(case_id) = &outcome.request.case_id {
            crate::server::sharepoint::sync_case_access(case_id.clone());
        }
    }
    if approve {
        let permission_change = match outcome.request.kind {
            AdminRequestKind::Role => outcome
                .request
                .requested_role
                .map(|role| format!("changed your account role to {}", role.label())),
            AdminRequestKind::InformationAccess => {
                outcome.request.requested_information_access.map(|enabled| {
                    if enabled {
                        "granted you access to Contacts, Organizations, and Funding".to_string()
                    } else {
                        "revoked your access to Contacts, Organizations, and Funding".to_string()
                    }
                })
            }
            AdminRequestKind::CaseCapabilities => None,
        };
        if let Some(change) = permission_change {
            crate::server::notifications::notify_account_permissions_changed(
                outcome.request.target_user_id.clone(),
                actor.full_name(),
                change,
            );
        }
    }
    crate::server::notifications::notify_admin_request_decided(outcome.request.clone());
    Ok(outcome.request)
}

#[cfg(feature = "ssr")]
pub(crate) fn validate_note(note: String) -> Result<String, ServerFnError> {
    let note = note.trim().to_string();
    if note.chars().count() > 1_000 {
        Err(ServerFnError::new(
            "Notes must be 1,000 characters or fewer.",
        ))
    } else {
        Ok(note)
    }
}
