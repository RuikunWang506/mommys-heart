//! User/admin server functions: role changes and per-case capability
//! assignments. All require operations-admin permissions.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::server_fns::capabilities::{CaseAssignment, CaseCapability};
use crate::server_fns::pagination::Page;

/// The global account type a user has. This controls app-level access (e.g.
/// operations and site admins can reach the Admin dashboard). It is largely
/// separate from per-case capabilities: what differs per case is a user's set of
/// [`CaseCapability`](crate::server_fns::capabilities::CaseCapability)s. The one
/// exception is [`AccountRole::SiteAdmin`], which holds every capability on
/// every case (see [`AccountRole::has_full_case_access`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountRole {
    /// A client the organization is helping.
    Client,
    /// A volunteer working cases on behalf of clients.
    Volunteer,
    /// An operations administrator who may manage their own case access and
    /// request changes to other users.
    OperationsAdmin,
    /// A site administrator with unrestricted administrative access.
    SiteAdmin,
    /// An account retired by a site admin. It grants nothing: it cannot sign in,
    /// and every permission gate denies it. The role it held before, and who
    /// retired it, live in `account_deactivations`.
    ///
    /// Deliberately a role rather than a flag: nearly every role-aware query
    /// matches positively (`role = 'volunteer'`, `role IN (...)`), so a
    /// deactivated account drops out of those lists by construction rather than
    /// relying on each query to remember an extra filter.
    Deactivated,
}

impl AccountRole {
    pub const ALL: [AccountRole; 5] = [
        AccountRole::Client,
        AccountRole::Volunteer,
        AccountRole::OperationsAdmin,
        AccountRole::SiteAdmin,
        AccountRole::Deactivated,
    ];

    /// The roles an admin may pick in the account-role dropdown.
    ///
    /// [`AccountRole::Deactivated`] is excluded on purpose, mirroring
    /// `CaseStatus::Withdrawn`'s absence from the staff status dropdown: the
    /// transition records who, when, and why, so it is reachable only through
    /// the dedicated server function, never as a stray dropdown change.
    pub const ASSIGNABLE: [AccountRole; 4] = [
        AccountRole::Client,
        AccountRole::Volunteer,
        AccountRole::OperationsAdmin,
        AccountRole::SiteAdmin,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AccountRole::Client => "Client",
            AccountRole::Volunteer => "Volunteer",
            AccountRole::OperationsAdmin => "Operations admin",
            AccountRole::SiteAdmin => "Site admin",
            AccountRole::Deactivated => "Deactivated",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            AccountRole::Client => "client",
            AccountRole::Volunteer => "volunteer",
            AccountRole::OperationsAdmin => "operations_admin",
            AccountRole::SiteAdmin => "site_admin",
            AccountRole::Deactivated => "deactivated",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.slug() == s)
    }

    /// Whether this is exactly the site-administrator role.
    pub fn is_site_admin(self) -> bool {
        matches!(self, AccountRole::SiteAdmin)
    }

    /// Whether this is exactly the operations-administrator role.
    pub fn is_operations_admin(self) -> bool {
        matches!(self, AccountRole::OperationsAdmin)
    }

    /// Whether this account has been retired and grants nothing.
    pub fn is_deactivated(self) -> bool {
        matches!(self, AccountRole::Deactivated)
    }

    /// Whether this role may perform operations-admin actions.
    /// Site administrators inherit these permissions.
    pub fn has_operations_admin_permissions(self) -> bool {
        self.is_operations_admin() || self.is_site_admin()
    }

    /// Whether this role holds every [`CaseCapability`] on every case without a
    /// stored assignment. Site admins do; everyone else, operations admins
    /// included, holds only what `case_assignments` records for them.
    pub fn has_full_case_access(self) -> bool {
        self.is_site_admin()
    }

    pub fn has_volunteer_privileges(self) -> bool {
        matches!(
            self,
            AccountRole::Volunteer | AccountRole::OperationsAdmin | AccountRole::SiteAdmin
        )
    }

    /// The SQL role list matching [`has_volunteer_privileges`], for the queries
    /// that must decide "is this account staff?" in the database.
    ///
    /// Written positively and kept in one place on purpose: a negative test
    /// would quietly count a deactivated account as staff.
    ///
    /// [`has_volunteer_privileges`]: AccountRole::has_volunteer_privileges
    pub const STAFF_ROLES_SQL: &'static str = "('volunteer', 'operations_admin', 'site_admin')";

    pub fn badge_classes(self) -> &'static str {
        match self {
            AccountRole::SiteAdmin => "bg-[#e3effa] text-[#1a5d99]",
            AccountRole::OperationsAdmin => "bg-[#fef3c7] text-[#7a4a06]",
            AccountRole::Volunteer => "bg-[#f3ece1] text-[#5b4f43]",
            AccountRole::Client => "bg-[#e3f5ec] text-[#11633f]",
            AccountRole::Deactivated => "bg-slate-950 text-slate-500 ring-1 ring-slate-700",
        }
    }
}

/// A user summary
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserSummary {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub role: AccountRole,
    /// Stored grant for the shared Contacts, Organizations, and Funding areas.
    pub information_management_access: bool,
}

impl UserSummary {
    /// Convenience: the user's full display name.
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }

    /// Whether this account's role and stored grant both allow information access.
    pub fn has_information_management_access(&self) -> bool {
        self.role.has_volunteer_privileges() && self.information_management_access
    }
}

/// Whether a volunteer has completed the volunteer agreement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgreementStatus {
    /// They accepted a specific version of the volunteer agreement.
    Completed,
    /// No accepted agreement on file. True for volunteers who predate the
    /// agreement, who were backfilled by migration.
    Outstanding,
}

impl AgreementStatus {
    pub fn label(self) -> &'static str {
        match self {
            AgreementStatus::Completed => "Completed",
            AgreementStatus::Outstanding => "Outstanding",
        }
    }

    pub fn badge_classes(self) -> &'static str {
        match self {
            AgreementStatus::Completed => {
                "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
            }
            AgreementStatus::Outstanding => {
                "bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30"
            }
        }
    }
}

/// One row of the admin "Volunteers" list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolunteerListItem {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub agreement: AgreementStatus,
}

impl VolunteerListItem {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }
}

/// Why an account is deactivated, and what to restore it to.
///
/// Modelled on [`VolunteerApplication`](crate::server_fns::volunteers::VolunteerApplication):
/// timestamps arrive already formatted for display, and the actor's name is a
/// stored snapshot so a later change to their account cannot erase the record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AccountDeactivation {
    /// The role held before deactivation, restored on reactivation.
    pub previous_role: AccountRole,
    /// Optional context the admin typed. Never required.
    pub reason: String,
    /// Display name of the admin who deactivated the account.
    pub by: String,
    /// When it happened, formatted for display.
    pub at: String,
}

/// Which exact account-role section of the grouped admin user directory to load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserDirectoryRoleGroup {
    Volunteer,
    Client,
    Other,
    /// Retired accounts, kept out of the three working sections so a duplicate
    /// stops cluttering them but stays findable and reversible.
    Deactivated,
}

/// One narrow row of the grouped admin user directory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserDirectoryItem {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub role: AccountRole,
    /// Volunteer-agreement status for exact volunteer accounts only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agreement: Option<AgreementStatus>,
    /// Distinct cases this user can access.
    pub assignment_count: i64,
    /// Concise human-readable summary of that access.
    pub assignment_summary: String,
}

impl UserDirectoryItem {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }
}

/// An application user account
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub home_address: String,
    pub role: AccountRole,
    /// Stored grant for the shared Contacts, Organizations, and Funding areas.
    pub information_management_access: bool,
    /// Cases this user is assigned to, with the capabilities they hold on each.
    #[serde(default)]
    pub assigned_cases: Vec<CaseAssignment>,
    /// Present exactly when this account holds [`AccountRole::Deactivated`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deactivation: Option<AccountDeactivation>,
}

impl User {
    /// Convenience: the user's full display name.
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }

    /// Whether this account's role and stored grant both allow information access.
    pub fn has_information_management_access(&self) -> bool {
        self.role.has_volunteer_privileges() && self.information_management_access
    }

    /// This user's capabilities on a given case: every capability for a role
    /// with full case access, else the stored assignment (empty if unassigned).
    pub fn capabilities_for(&self, case_id: &str) -> Vec<CaseCapability> {
        if self.role.has_full_case_access() {
            return CaseCapability::ALL.to_vec();
        }
        self.assigned_cases
            .iter()
            .find(|a| a.case_id == case_id)
            .map(|a| a.capabilities.clone())
            .unwrap_or_default()
    }

    /// Whether this user is assigned to the given case at all.
    pub fn is_assigned_to(&self, case_id: &str) -> bool {
        self.assigned_cases.iter().any(|a| a.case_id == case_id)
    }

    /// Whether this account has been retired.
    pub fn is_deactivated(&self) -> bool {
        self.role.is_deactivated()
    }
}

impl From<User> for UserSummary {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            first_name: user.first_name,
            last_name: user.last_name,
            role: user.role,
            information_management_access: user.information_management_access,
        }
    }
}

/// Server-side typeahead search over users for the case owner-picker. Requires
/// operations-admin permissions because it returns other users' confidential
/// names and its sole consumer is owner reassignment.
#[server(prefix = "/api")]
pub async fn search_users(query: String) -> Result<Vec<UserSummary>, ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    users::search_user_summaries(&query, 10)
        .await
        .map_err(ServerFnError::new)
}

/// One page of users for the admin management screen, ordered by id, with an
/// optional case-insensitive search over id/name/email. Requires
/// operations-admin permissions.
///
/// Backs the admin dashboard's server-side pagination ("Load more") so the UI
/// never has to pull every user into the browser.
#[server(prefix = "/api")]
pub async fn list_users_page(
    offset: i64,
    limit: i64,
    search: String,
) -> Result<Page<User>, ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    users::page(offset, limit, &search)
        .await
        .map_err(ServerFnError::new)
}

/// One page of volunteer accounts for the admin "Volunteers" tab, with the same
/// optional search as [`list_users_page`]. Requires operations-admin permissions.
#[server(prefix = "/api")]
pub async fn list_volunteers_page(
    offset: i64,
    limit: i64,
    search: String,
) -> Result<Page<VolunteerListItem>, ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    users::volunteers_page(
        offset,
        limit,
        &search,
        crate::helpers::volunteer_terms::VOLUNTEER_AGREEMENT_VERSION,
    )
    .await
    .map_err(ServerFnError::new)
}

/// One grouped section of the admin user directory, using a narrow row DTO so
/// contact details and full assignments are fetched only when needed.
#[server(prefix = "/api")]
pub async fn list_user_directory_page(
    offset: i64,
    limit: i64,
    search: String,
    role_group: UserDirectoryRoleGroup,
) -> Result<Page<UserDirectoryItem>, ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    users::directory_page(
        offset,
        limit,
        &search,
        role_group,
        crate::helpers::volunteer_terms::VOLUNTEER_AGREEMENT_VERSION,
    )
    .await
    .map_err(ServerFnError::new)
}

/// Lazily load one full user record for the admin management view.
#[server(prefix = "/api")]
pub async fn load_admin_user(user_id: String) -> Result<User, ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    let user_id = user_id.trim().to_string();
    if user_id.is_empty() {
        return Err(ServerFnError::new("No user was requested."));
    }
    users::get(&user_id)
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| ServerFnError::new("User not found."))
}

/// Change a user's account role.
#[server(prefix = "/api")]
pub async fn set_user_role(user_id: String, role: AccountRole) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    // Deactivation carries who, when, and why, so it goes through its own
    // function rather than arriving as an ordinary role change.
    if role.is_deactivated() {
        return Err(ServerFnError::new(
            "Use the account status controls to deactivate an account.",
        ));
    }
    let user_id = user_id.trim().to_string();
    let changed = users::set_role(&user_id, role, &actor.full_name())
        .await
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("You cannot demote the final site admin.") {
                ServerFnError::new("You cannot demote the final site admin.")
            } else {
                ServerFnError::new(error)
            }
        })?;
    if changed {
        crate::server::notifications::notify_account_permissions_changed(
            user_id.clone(),
            actor.full_name(),
            format!("changed your account role to {}", role.label()),
        );
        // A role change moves the volunteer-only line: a volunteer demoted to
        // client must lose those folders, and a client promoted must gain them.
        // The audience is an account-role gate, so nothing else would catch it.
        sync_document_access_for_user(&user_id).await;
    }
    Ok(())
}

/// Change a user's primary (sign-in) email address. Site admin only, and the
/// address must be typed twice — both copies are sent so the confirmation is
/// enforced on the server, not just in the browser.
///
/// The account keeps its password and sessions; only the address it signs in
/// with changes. Both the old and new addresses are notified.
#[server(prefix = "/api")]
pub async fn set_user_email(
    user_id: String,
    new_email: String,
    confirm_new_email: String,
) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_site_admin, require_user};
    use crate::server_fns::auth::validate_confirmed_email;

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    let user_id = user_id.trim().to_string();
    if user_id.is_empty() {
        return Err(ServerFnError::new("No account was chosen."));
    }
    let new_email = validate_confirmed_email(&new_email, &confirm_new_email)?;

    let previous = users::set_email(&user_id, &new_email, &actor.id, &actor.full_name())
        .await
        .map_err(|error| ServerFnError::new(error.to_string()))?;
    // `None` means it already matched, so there is nothing to announce.
    if let Some(previous) = previous {
        crate::server::notifications::notify_email_changed(user_id, actor.full_name(), previous);
    }
    Ok(())
}

/// Retire an account, or restore a retired one. Site admin only.
///
/// The account keeps everything — role, case assignments, volunteer agreement,
/// audit history — so a reactivation is lossless. What it loses is the ability
/// to sign in and its place in every list and picker.
#[server(prefix = "/api")]
pub async fn set_account_deactivated(
    user_id: String,
    deactivated: bool,
    reason: String,
) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    let user_id = user_id.trim().to_string();
    if user_id.is_empty() {
        return Err(ServerFnError::new("No account was requested."));
    }
    // Locking yourself out is never the intent, and there would be no one left
    // in this browser to undo it.
    if user_id == actor.id {
        return Err(ServerFnError::new(
            "You cannot deactivate your own account.",
        ));
    }
    let reason = reason.trim();
    if reason.chars().count() > 1000 {
        return Err(ServerFnError::new(
            "Please keep the reason under 1000 characters.",
        ));
    }

    let changed = users::set_deactivated(
        &user_id,
        deactivated,
        reason,
        &actor.id,
        &actor.full_name(),
    )
    .await
    .map_err(|error| {
        // Surface the domain refusals verbatim; anything else is a real fault.
        let message = error.to_string();
        if message.contains("You cannot demote the final site admin.") {
            ServerFnError::new("You cannot deactivate the final site admin.")
        } else if message.contains("no deactivation record") {
            ServerFnError::new("This account has no deactivation record to restore from.")
        } else {
            ServerFnError::new(error)
        }
    })?;

    if changed && !deactivated {
        // Only worth telling someone whose account just came back.
        crate::server::notifications::notify_account_permissions_changed(
            user_id.clone(),
            actor.full_name(),
            "reactivated your account".to_string(),
        );
    }
    if changed {
        // A retired account keeps its assignments, so the cases to re-sync have
        // to come from what was actually granted rather than from capabilities.
        // Deactivating withdraws every invitation; reactivating restores them.
        sync_document_access_for_user(&user_id).await;
    }
    Ok(())
}

/// Re-reconcile document sharing on every case a user is involved with.
///
/// Used when something about the *account* changes rather than a single
/// assignment — deactivation and reactivation — where the set of affected cases
/// is not named by the request.
#[cfg(feature = "ssr")]
async fn sync_document_access_for_user(user_id: &str) {
    use crate::server::db::case_documents;

    // Cases they still hold an invitation on, plus cases they are assigned to:
    // the first covers revoking, the second covers restoring.
    let mut case_ids = case_documents::case_ids_granted_to(user_id)
        .await
        .unwrap_or_default();
    if let Ok(user) = crate::server::db::users::get(user_id).await {
        if let Some(user) = user {
            for assignment in &user.assigned_cases {
                if !case_ids.contains(&assignment.case_id) {
                    case_ids.push(assignment.case_id.clone());
                }
            }
        }
    }
    for case_id in case_ids {
        crate::server::sharepoint::sync_case_access(case_id);
    }
}

/// Grant or revoke access to Contacts, Organizations, and Funding information.
#[server(prefix = "/api")]
pub async fn set_information_management_access(
    user_id: String,
    enabled: bool,
) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    let user_id = user_id.trim().to_string();
    let changed =
        users::set_information_management_access(&user_id, enabled, &actor.id, &actor.full_name())
            .await
            .map_err(ServerFnError::new)?;
    if changed {
        let change = if enabled {
            "granted you access to Contacts, Organizations, and Funding"
        } else {
            "revoked your access to Contacts, Organizations, and Funding"
        };
        crate::server::notifications::notify_account_permissions_changed(
            user_id,
            actor.full_name(),
            change.to_string(),
        );
    }
    Ok(())
}

/// Assign a user to a case with an explicit set of capabilities.
#[server(prefix = "/api")]
pub async fn assign_case(
    user_id: String,
    case_id: String,
    capabilities: Vec<CaseCapability>,
) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_case_access_management, require_user};
    use crate::server_fns::capabilities::validate_capabilities;

    let actor = require_user().await?;
    require_case_access_management(&actor, &user_id)?;
    validate_capabilities(&capabilities).map_err(ServerFnError::new)?;
    let was_assigned = users::is_assigned(&user_id, &case_id).await.unwrap_or(true);
    users::assign_capabilities(&user_id, &case_id, &capabilities, &actor.full_name())
        .await
        .map_err(ServerFnError::new)?;
    if !was_assigned && capabilities.contains(&CaseCapability::ViewCase) {
        crate::server::notifications::notify_assignment(
            user_id.clone(),
            actor.full_name(),
            case_id.clone(),
        );
    }
    // Bring the document library's sharing in line with the new capability set.
    crate::server::sharepoint::sync_case_access(case_id);
    Ok(())
}

/// Toggle a single capability for a user on a case.
#[server(prefix = "/api")]
pub async fn toggle_capability(
    user_id: String,
    case_id: String,
    capability: CaseCapability,
    enabled: bool,
) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_case_access_management, require_user};

    let actor = require_user().await?;
    require_case_access_management(&actor, &user_id)?;
    users::toggle_capability(&user_id, &case_id, capability, enabled, &actor.full_name())
        .await
        .map_err(ServerFnError::new)?;
    crate::server::sharepoint::sync_case_access(case_id);
    Ok(())
}

/// Remove a user's assignment to a case entirely.
#[server(prefix = "/api")]
pub async fn unassign_case(user_id: String, case_id: String) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_case_access_management, require_user};

    let actor = require_user().await?;
    require_case_access_management(&actor, &user_id)?;
    users::unassign(&user_id, &case_id, &actor.full_name())
        .await
        .map_err(ServerFnError::new)?;
    // Withdraws every invitation this user held on the case.
    crate::server::sharepoint::sync_case_access(case_id);
    Ok(())
}

/// Apply a batch of per-case capability changes for one user in a single
/// request. Each change is either a new capability set for a case (`Some`) or a
/// removal of the assignment (`None`). Backs the admin "Edit → Save" flow so a
/// whole draft applies in one round-trip instead of one server call per checkbox.
#[server(prefix = "/api", input = leptos::server_fn::codec::Json)]
pub async fn save_case_capabilities(
    user_id: String,
    changes: Vec<(String, Option<Vec<CaseCapability>>)>,
) -> Result<(), ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::{require_case_access_management, require_user};
    use crate::server_fns::capabilities::validate_capabilities;

    let actor = require_user().await?;
    require_case_access_management(&actor, &user_id)?;
    for (_, caps) in &changes {
        if let Some(caps) = caps {
            validate_capabilities(caps).map_err(ServerFnError::new)?;
        }
    }
    let actor_name = actor.full_name();
    for (case_id, caps) in changes {
        match caps {
            Some(caps) => {
                let was_assigned = users::is_assigned(&user_id, &case_id).await.unwrap_or(true);
                users::assign_capabilities(&user_id, &case_id, &caps, &actor_name)
                    .await
                    .map_err(ServerFnError::new)?;
                if !was_assigned && caps.contains(&CaseCapability::ViewCase) {
                    crate::server::notifications::notify_assignment(
                        user_id.clone(),
                        actor_name.clone(),
                        case_id.clone(),
                    );
                }
            }
            None => users::unassign(&user_id, &case_id, &actor_name)
                .await
                .map_err(ServerFnError::new)?,
        }
        // Every branch changes what this user may reach, so both grants and
        // revocations are reconciled the same way.
        crate::server::sharepoint::sync_case_access(case_id);
    }
    Ok(())
}
