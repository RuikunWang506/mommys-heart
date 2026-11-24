//! Contacts: the people the foundation knows.
//!
//! The distinction that makes this a CRM: a [`crate::server_fns::users::User`] is
//! an *account* that can sign in; a [`Contact`] is a *person* the organization
//! has a relationship with. Most contacts never get an account — donors, funder
//! program officers, partner caseworkers, opposing attorneys, emergency
//! contacts — so they cannot be represented as users, which require a unique
//! email and a password hash.
//!
//! A contact may link to at most one user. When it does, `users` stays the source
//! of truth for identity, email, and role: this record shows them read-only and
//! never stores a second copy or a second login path.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::server_fns::crm::{clean_text, coded_enum, MAX_LONG_TEXT, MAX_NAME, MAX_SHORT_TEXT};
use crate::server_fns::pagination::Page;
use crate::server_fns::users::AccountRole;

/// How many types one contact may carry, so a submitted list cannot grow without
/// bound.
pub const MAX_TYPES: usize = 6;

coded_enum!(ContactType {
    Client => ("client", "Client"),
    Volunteer => ("volunteer", "Volunteer"),
    Staff => ("staff", "Staff"),
    Donor => ("donor", "Donor"),
    FunderContact => ("funder_contact", "Funder contact"),
    Partner => ("partner", "Partner"),
    ServiceProvider => ("service_provider", "Service provider"),
    Attorney => ("attorney", "Attorney"),
    CourtProfessional => ("court_professional", "Court professional"),
    GovernmentAgency => ("government_agency", "Government agency"),
    EmergencyContact => ("emergency_contact", "Emergency contact"),
    BoardMember => ("board_member", "Board member"),
    Vendor => ("vendor", "Vendor"),
    Other => ("other", "Other"),
});

impl ContactType {
    pub fn badge_classes(self) -> &'static str {
        match self {
            Self::Client => "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30",
            Self::Volunteer => "bg-sky-500/15 text-sky-300 ring-1 ring-sky-500/30",
            Self::Staff => "bg-primary-500/15 text-primary-300 ring-1 ring-primary-500/30",
            Self::Donor | Self::FunderContact => {
                "bg-amber-500/15 text-amber-300 ring-1 ring-amber-500/30"
            }
            _ => "bg-slate-700/40 text-slate-300 ring-1 ring-slate-600",
        }
    }
}

/// A person in the directory. `organization_name` and the `linked_*` fields are
/// joined in for display; they are owned by their own tables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub preferred_name: String,
    pub email: String,
    pub phone: String,
    pub mobile: String,
    pub address: String,
    pub job_title: String,
    pub organization_id: String,
    pub organization_name: String,
    pub types: Vec<ContactType>,
    #[serde(default)]
    pub organization_archived: bool,
    pub source: String,
    pub description: String,
    pub do_not_contact: bool,
    pub archived: bool,
    /// The account this person signs in with, when they have one.
    pub user_id: String,
    /// Whether account-owned identity fields are locked, even when account
    /// identifiers and private values are withheld from this viewer.
    #[serde(default)]
    pub account_linked: bool,
    /// Read-only snapshots of the linked account, owned by `users`.
    pub linked_email: String,
    pub linked_role: Option<AccountRole>,
    /// Preserved contact values differ from account-owned projected fields.
    #[serde(default)]
    pub has_account_field_conflict: bool,
    #[serde(default)]
    pub openable_profile_id: String,
}

impl Contact {
    /// The name to show: their preferred name when they gave one, else first and
    /// last, else the organization they belong to.
    pub fn display_name(&self) -> String {
        let given = if self.preferred_name.trim().is_empty() {
            self.first_name.trim()
        } else {
            self.preferred_name.trim()
        };
        let full = format!("{} {}", given, self.last_name.trim())
            .trim()
            .to_string();
        if full.is_empty() {
            self.organization_name.clone()
        } else {
            full
        }
    }

    pub fn has_account(&self) -> bool {
        self.account_linked || !self.user_id.is_empty()
    }
}

/// Normalize one contact's broad CRM classifications and enforce the shared
/// storage limit.
pub fn validate_contact_types(input: &[ContactType]) -> Result<Vec<ContactType>, String> {
    let mut types = Vec::new();
    for contact_type in input {
        if !types.contains(contact_type) {
            types.push(*contact_type);
        }
    }
    if types.is_empty() {
        return Err("Choose at least one contact type.".into());
    }
    if types.len() > MAX_TYPES {
        return Err(format!("Choose at most {MAX_TYPES} contact types."));
    }
    Ok(types)
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContactInput {
    pub first_name: String,
    pub last_name: String,
    pub preferred_name: String,
    pub email: String,
    pub phone: String,
    pub mobile: String,
    pub address: String,
    pub job_title: String,
    /// Empty means "not filed under an organization".
    pub organization_id: String,
    pub types: Vec<ContactType>,
    pub source: String,
    pub description: String,
    pub do_not_contact: bool,
}

impl ContactInput {
    /// Trim every field, drop duplicate types, and reject what cannot be stored.
    /// Mirrors the database `CHECK`s so a bad value produces a readable message
    /// rather than a constraint violation.
    pub fn validate(&self) -> Result<Self, String> {
        let types = validate_contact_types(&self.types)?;

        let cleaned = Self {
            first_name: clean_text(&self.first_name, "first name", MAX_NAME)?,
            last_name: clean_text(&self.last_name, "last name", MAX_NAME)?,
            preferred_name: clean_text(&self.preferred_name, "preferred name", MAX_NAME)?,
            email: clean_text(&self.email, "email", MAX_SHORT_TEXT)?,
            phone: clean_text(&self.phone, "phone", MAX_SHORT_TEXT)?,
            mobile: clean_text(&self.mobile, "mobile", MAX_SHORT_TEXT)?,
            address: clean_text(&self.address, "address", MAX_SHORT_TEXT)?,
            job_title: clean_text(&self.job_title, "job title", MAX_NAME)?,
            organization_id: self.organization_id.trim().to_string(),
            types,
            source: clean_text(&self.source, "source", MAX_SHORT_TEXT)?,
            description: clean_text(&self.description, "description", MAX_LONG_TEXT)?,
            do_not_contact: self.do_not_contact,
        };

        // Matches `contacts_named_check`: a row with neither a surname nor an
        // employer has nothing to show in a directory.
        if cleaned.last_name.is_empty() && cleaned.organization_id.is_empty() {
            return Err("Enter a last name, or file this contact under an organization.".into());
        }
        if !cleaned.email.is_empty() && !cleaned.email.contains('@') {
            return Err("Enter a valid email address.".into());
        }
        Ok(cleaned)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ContactFilters {
    /// Matched against name, email, phone, and organization name.
    pub keyword: String,
    pub contact_type: Option<ContactType>,
    pub organization_id: String,
    pub include_archived: bool,
}

/// One narrow active contact option for server-side typeahead pickers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActiveContactSummary {
    pub id: String,
    pub label: String,
    pub organization_id: String,
    pub organization_name: String,
}

#[server(prefix = "/api")]
pub async fn list_contacts(
    filters: ContactFilters,
    offset: i64,
    limit: i64,
) -> Result<Page<Contact>, ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    crate::server_fns::crm::require_staff(&user)?;
    contacts::page(
        &filters,
        offset,
        limit,
        user.role.has_operations_admin_permissions(),
    )
    .await
    .map_err(ServerFnError::new)
}

#[server(prefix = "/api")]
pub async fn load_contact(id: String) -> Result<Option<Contact>, ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    crate::server_fns::crm::require_staff(&user)?;
    let is_operations_admin = user.role.has_operations_admin_permissions();
    let Some(mut contact) = contacts::get(&id, is_operations_admin)
        .await
        .map_err(ServerFnError::new)?
    else {
        return Ok(None);
    };
    if let Some(account_id) = contacts::account_of(&id).await.map_err(ServerFnError::new)? {
        let can_open = is_operations_admin
            || account_id == user.id
            || crate::server::db::users::shares_case(&user.id, &account_id)
                .await
                .map_err(ServerFnError::new)?;
        if can_open {
            contact.openable_profile_id = account_id;
        }
    }
    Ok(Some(contact))
}

/// Server-side typeahead search over active contacts for relationship pickers.
#[server(prefix = "/api")]
pub async fn search_active_contacts(
    query: String,
) -> Result<Vec<ActiveContactSummary>, ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    crate::server_fns::crm::require_staff(&user)?;
    contacts::search_active(&query, 10)
        .await
        .map_err(ServerFnError::new)
}

/// Creating and editing a person requires the shared information-management grant.
#[server(prefix = "/api")]
pub async fn create_contact(input: ContactInput) -> Result<String, ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    let input = input.validate().map_err(ServerFnError::new)?;
    contacts::create(&input, &user.id, &user.full_name())
        .await
        .map_err(ServerFnError::new)
}

#[server(prefix = "/api")]
pub async fn update_contact(id: String, input: ContactInput) -> Result<(), ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    let input = input.validate().map_err(ServerFnError::new)?;
    contacts::update(&id, &input, &user.id, &user.full_name())
        .await
        .map_err(ServerFnError::new)
}

/// Archiving removes someone from every picker and requires information access.
#[server(prefix = "/api")]
pub async fn set_contact_archived(id: String, archived: bool) -> Result<(), ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    contacts::set_archived(&id, archived, &user.id, &user.full_name())
        .await
        .map_err(ServerFnError::new)
}

/// One account that could be linked to a person, for the link picker.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LinkableAccount {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// Accounts that have no person record yet.
///
/// Only these can be linked, so linking can never point two people at one
/// account or silently steal an account from another contact.
#[server(prefix = "/api")]
pub async fn unlinked_accounts() -> Result<Vec<LinkableAccount>, ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::{require_operations_admin, require_user};

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    require_operations_admin(&user)?;
    contacts::unlinked_accounts()
        .await
        .map_err(ServerFnError::new)
}

/// Link a contact to an existing account, or unlink it when `user_id` is empty.
///
/// Never touches the account itself: unlinking leaves the user able to sign in
/// exactly as before, and the person record survives independently.
#[server(prefix = "/api")]
pub async fn set_contact_account(id: String, user_id: String) -> Result<(), ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::{require_operations_admin, require_user};

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    require_operations_admin(&user)?;
    let target = user_id.trim();
    contacts::set_account(
        &id,
        (!target.is_empty()).then_some(target),
        &user.id,
        &user.full_name(),
    )
    .await
    .map_err(ServerFnError::new)
}

/// File a contact under an organization, move them between organizations, or
/// remove them from one when the contact can still be named without it.
#[server(prefix = "/api")]
pub async fn set_contact_organization(
    contact_id: String,
    organization_id: String,
) -> Result<(), ServerFnError> {
    use crate::server::db::contacts;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    crate::server::permissions::require_information_management_access(&user)?;
    let target = organization_id.trim();
    contacts::set_organization(
        &contact_id,
        (!target.is_empty()).then_some(target),
        &user.id,
        &user.full_name(),
    )
    .await
    .map_err(ServerFnError::new)
}
