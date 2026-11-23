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
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DuplicateReport {
    pub case_groups: Vec<DuplicateCaseGroup>,
    pub case_groups_truncated: bool,
    pub contact_groups: Vec<DuplicateContactGroup>,
    pub contact_groups_truncated: bool,
    /// False when the caller may not see contacts, so the UI can say why that
    /// list is missing rather than claim there are no duplicate contacts.
    pub contacts_checked: bool,
}

/// Find cases and contacts that may be duplicates.
///
/// Cases are listed for anyone who can see the admin case directory (which
/// already shows every case's name). Contacts are only checked for callers who
/// may see the contact directory.
#[server(prefix = "/api")]
pub async fn possible_duplicates() -> Result<DuplicateReport, ServerFnError> {
    use crate::server::db::duplicates;
    use crate::server::permissions::{
        has_information_management_access, require_operations_admin, require_user,
    };

    let user = require_user().await?;
    require_operations_admin(&user)?;

    let (case_groups, case_groups_truncated) = duplicates::case_groups(MAX_GROUPS)
        .await
        .map_err(ServerFnError::new)?;

    let contacts_checked = has_information_management_access(&user);
    let (contact_groups, contact_groups_truncated) = if contacts_checked {
        duplicates::contact_groups(MAX_GROUPS)
            .await
            .map_err(ServerFnError::new)?
    } else {
        (Vec::new(), false)
    };

    Ok(DuplicateReport {
        case_groups,
        case_groups_truncated,
        contact_groups,
        contact_groups_truncated,
        contacts_checked,
    })
}
