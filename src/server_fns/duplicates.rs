//! Possible duplicate records: cases that share a name, and contacts that share
//! a name, email address, or phone number.
//!
//! This only *finds* candidates. Nothing is merged or deleted automatically:
//! two clients can genuinely share a name, so a person who knows the cases
//! opens each record, decides which one to keep, and withdraws or archives the
//! other with the tools that already exist.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::server_fns::cases::CaseStatus;
use crate::server_fns::contacts::ContactType;

/// The most groups returned per list, so a large directory cannot produce an
/// unbounded page. The UI says when the list was cut short.
pub const MAX_GROUPS: usize = 100;

/// One case in a group of cases with the same name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuplicateCase {
    pub id: String,
    pub name: String,
    pub status: CaseStatus,
    /// The account the case is filed under.
    pub filed_by: String,
    /// Contacts linked to the case with the Client role.
    pub clients: Vec<String>,
    /// Case notes that were not discarded.
    pub note_count: i64,
    pub message_count: i64,
    /// When the latest chat message was sent, if any (`YYYY-MM-DD HH:MM`).
    pub last_message: Option<String>,
}

/// Cases whose names match once case and spacing are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuplicateCaseGroup {
    pub cases: Vec<DuplicateCase>,
    /// Someone marked this exact set of cases as not duplicates.
    #[serde(default)]
    pub dismissed: bool,
}

impl DuplicateCaseGroup {
    pub fn ids(&self) -> Vec<String> {
        self.cases.iter().map(|case| case.id.clone()).collect()
    }
}

/// Why contacts were grouped together.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MatchReason {
    Name,
    Email,
    Phone,
}

impl MatchReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Same name",
            Self::Email => "Same email",
            Self::Phone => "Same phone number",
        }
    }
}

/// One contact in a group of contacts that may be the same person.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuplicateContact {
    pub id: String,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub types: Vec<ContactType>,
    /// Cases this contact is linked to, in any role.
    pub case_count: i64,
    pub has_account: bool,
    /// Last change to the record (`YYYY-MM-DD`).
    pub updated_on: String,
}

/// Contacts connected by at least one shared name, email, or phone number.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuplicateContactGroup {
    pub reasons: Vec<MatchReason>,
    pub contacts: Vec<DuplicateContact>,
    /// Someone marked this exact set of contacts as not duplicates.
    #[serde(default)]
    pub dismissed: bool,
}

impl DuplicateContactGroup {
    pub fn ids(&self) -> Vec<String> {
        self.contacts
            .iter()
            .map(|contact| contact.id.clone())
            .collect()
    }
}

/// Which list a "not duplicates" mark belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuplicateKind {
    Case,
    Contact,
}

impl DuplicateKind {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Case => "case",
            Self::Contact => "contact",
        }
    }
}

/// A group's ids in the form they are stored and compared in: sorted, unique.
pub fn normalized_ids(ids: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = ids
        .iter()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DuplicateReport {
    pub case_groups: Vec<DuplicateCaseGroup>,
    pub case_groups_truncated: bool,
    pub contact_groups: Vec<DuplicateContactGroup>,
    pub contact_groups_truncated: bool,
    /// How many groups are hidden because they were marked not duplicates.
    #[serde(default)]
    pub dismissed_case_groups: usize,
    #[serde(default)]
    pub dismissed_contact_groups: usize,
    /// False when the caller may not see contacts, so the UI can say why that
    /// list is missing rather than claim there are no duplicate contacts.
    pub contacts_checked: bool,
}

/// Find cases and contacts that may be duplicates.
///
/// Cases are listed for anyone who can see the admin case directory (which
/// already shows every case's name). Contacts are only checked for callers who
/// may see the contact directory.
///
/// Groups marked "not duplicates" are left out unless `include_dismissed` is
/// set, in which case they are returned with `dismissed` set so they can be
/// restored.
#[server(prefix = "/api")]
pub async fn possible_duplicates(
    include_dismissed: bool,
) -> Result<DuplicateReport, ServerFnError> {
    use crate::server::db::duplicates;
    use crate::server::permissions::{require_operations_admin, require_user};

    let user = require_user().await?;
    require_operations_admin(&user)?;

    let dismissed_cases = duplicates::dismissed_sets(DuplicateKind::Case.slug())
        .await
        .map_err(ServerFnError::new)?;
    let mut case_groups = duplicates::case_groups()
        .await
        .map_err(ServerFnError::new)?;
    for group in &mut case_groups {
        group.dismissed = dismissed_cases.contains(&normalized_ids(&group.ids()));
    }
    let dismissed_case_groups = case_groups.iter().filter(|g| g.dismissed).count();
    if !include_dismissed {
        case_groups.retain(|g| !g.dismissed);
    }
    let case_groups_truncated = case_groups.len() > MAX_GROUPS;
    case_groups.truncate(MAX_GROUPS);

    let contacts_checked = can_check_contacts(&user);
    let mut contact_groups = Vec::new();
    let mut dismissed_contact_groups = 0;
    if contacts_checked {
        let dismissed_contacts = duplicates::dismissed_sets(DuplicateKind::Contact.slug())
            .await
            .map_err(ServerFnError::new)?;
        contact_groups = duplicates::contact_groups()
            .await
            .map_err(ServerFnError::new)?;
        for group in &mut contact_groups {
            group.dismissed = dismissed_contacts.contains(&normalized_ids(&group.ids()));
        }
        dismissed_contact_groups = contact_groups.iter().filter(|g| g.dismissed).count();
        if !include_dismissed {
            contact_groups.retain(|g| !g.dismissed);
        }
    }
    let contact_groups_truncated = contact_groups.len() > MAX_GROUPS;
    contact_groups.truncate(MAX_GROUPS);

    Ok(DuplicateReport {
        case_groups,
        case_groups_truncated,
        contact_groups,
        contact_groups_truncated,
        dismissed_case_groups,
        dismissed_contact_groups,
        contacts_checked,
    })
}

/// Whether the caller may see the contact half of the report (same rule as the directory).
#[cfg(feature = "ssr")]
fn can_check_contacts(user: &crate::server_fns::users::User) -> bool {
    crate::server::permissions::has_information_management_access(user)
}

/// Mark a group as not duplicates (`dismissed = true`), or list it again
/// (`dismissed = false`). Takes the group's record ids as shown.
#[server(prefix = "/api", input = leptos::server_fn::codec::Json)]
pub async fn set_duplicate_dismissed(
    kind: DuplicateKind,
    ids: Vec<String>,
    dismissed: bool,
) -> Result<(), ServerFnError> {
    use crate::server::db::duplicates;
    use crate::server::permissions::{require_operations_admin, require_user};

    let user = require_user().await?;
    require_operations_admin(&user)?;
    if kind == DuplicateKind::Contact && !can_check_contacts(&user) {
        return Err(ServerFnError::new(
            "Access to contacts, organizations, and funding information has not been granted.",
        ));
    }
    let ids = normalized_ids(&ids);
    if ids.len() < 2 || ids.len() > 200 {
        return Err(ServerFnError::new(
            "A group needs between 2 and 200 records.",
        ));
    }
    let result = if dismissed {
        duplicates::dismiss(kind.slug(), &ids, &user.id, &user.full_name()).await
    } else {
        duplicates::undismiss(kind.slug(), &ids).await
    };
    result.map_err(ServerFnError::new)
}
