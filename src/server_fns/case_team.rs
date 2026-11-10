//! Case-side assignment: see who is on a case and add or remove several people
//! at once. Reads and writes the same `case_assignments` rows as the user card.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::users::{AccountRole, UserSummary};

/// One person assigned to a case, with what they may do on it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseTeamMember {
    pub user_id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub role: AccountRole,
    pub capabilities: Vec<CaseCapability>,
}

impl CaseTeamMember {
    pub fn full_name(&self) -> String {
        let name = format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string();
        if name.is_empty() {
            self.email.clone()
        } else {
            name
        }
    }
}

/// Everyone assigned to a case. Requires operations-admin permissions.
#[server(prefix = "/api")]
pub async fn load_case_team(case_id: String) -> Result<Vec<CaseTeamMember>, ServerFnError> {
    use crate::server::db::case_team;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    case_team::members(case_id.trim())
        .await
        .map_err(ServerFnError::new)
}

/// Volunteers and operations admins who can be added to a case team.
#[server(prefix = "/api")]
pub async fn search_assignable_users(query: String) -> Result<Vec<UserSummary>, ServerFnError> {
    use crate::server::db::case_team;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    case_team::search_assignable(&query, 500)
        .await
        .map_err(ServerFnError::new)
}

/// Save several people's access to one case in one go. Each change is
/// `(user_id, Some(capabilities))` to set access or `(user_id, None)` to remove it.
///
/// Site admins apply everything directly. Operations admins apply their own row
/// directly and file the rest as requests for site-admin approval, matching the
/// user card. Returns a short message describing what happened.
#[server(prefix = "/api", input = leptos::server_fn::codec::Json)]
pub async fn save_case_team(
    case_id: String,
    changes: Vec<(String, Option<Vec<CaseCapability>>)>,
    note: String,
) -> Result<String, ServerFnError> {
    use crate::server::db::{admin_requests, case_team};
    use crate::server::permissions::{require_operations_admin, require_user};
    use crate::server_fns::admin_requests::validate_note;
    use crate::server_fns::capabilities::validate_capabilities;

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    let case_id = case_id.trim().to_string();
    if case_id.is_empty() {
        return Err(ServerFnError::new("Choose a case."));
    }
    if changes.is_empty() {
        return Err(ServerFnError::new("There are no changes to save."));
    }
    let mut seen = std::collections::HashSet::new();
    for (user_id, caps) in &changes {
        if !seen.insert(user_id.as_str()) {
            return Err(ServerFnError::new("Each person can appear only once."));
        }
        if let Some(caps) = caps {
            if caps.is_empty() {
                return Err(ServerFnError::new(
                    "Choose at least one permission, or remove the person.",
                ));
            }
            validate_capabilities(caps).map_err(ServerFnError::new)?;
        }
    }
    let note = validate_note(note)?;
    let actor_name = actor.full_name();

    // Operations admins may change only their own access directly.
    let (direct, requested): (Vec<_>, Vec<_>) = if actor.role.is_site_admin() {
        (changes, Vec::new())
    } else {
        changes
            .into_iter()
            .partition(|(user_id, _)| *user_id == actor.id)
    };

    if !requested.is_empty() {
        let rows = requested
            .iter()
            .map(|(user_id, caps)| (user_id.clone(), case_id.clone(), caps.clone()))
            .collect::<Vec<_>>();
        let filed = admin_requests::create_case_capability_requests(&actor.id, &rows, &note)
            .await
            .map_err(|error| ServerFnError::new(error.to_string()))?;
        for request in filed {
            crate::server::notifications::notify_admin_request_filed(request);
        }
    }

    if !direct.is_empty() {
        let newly_assigned = case_team::apply(&case_id, &direct, &actor_name)
            .await
            .map_err(|error| ServerFnError::new(error.to_string()))?;
        for user_id in newly_assigned {
            crate::server::notifications::notify_assignment(
                user_id,
                actor_name.clone(),
                case_id.clone(),
            );
        }
        // Bring the document library's sharing in line with the new team.
        crate::server::sharepoint::sync_case_access(case_id.clone());
    }

    Ok(match (direct.len(), requested.len()) {
        (_, 0) => "Case team updated.".to_string(),
        (0, 1) => "Request submitted for site-admin approval.".to_string(),
        (0, n) => format!("{n} requests submitted for site-admin approval."),
        (_, n) => format!(
            "Your own access was updated, and {n} request{} submitted for site-admin approval.",
            if n == 1 { " was" } else { "s were" }
        ),
    })
}
