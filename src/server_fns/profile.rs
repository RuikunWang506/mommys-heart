//! User profiles: the self-service "who am I" screen at `/profile` and the
//! read-only view of a colleague's profile at `/profile/:id`.
//!
//! Profiles are **need-to-know**, not public: a profile is only readable by its
//! owner, by a user with operations-admin permissions, or by someone who shares
//! a case with that user (both hold the `view_case` capability on the same case,
//! or own it).
//!
//! What a reader sees is narrower still. Name and role identify a colleague and
//! are shown to anyone who may open the profile at all; the contact details
//! (email, phone, home address) are personal information and are sent **only**
//! to the profile's owner and to users with operations-admin permissions. That
//! filtering happens server-side in [`load_profile`] — [`UserProfile::contact`]
//! is simply absent from the response for everyone else, so the details never
//! reach the browser.
//!
//! Editing is strictly self-service: [`save_my_profile`] always writes the
//! caller's own row. Account-level fields that are *not* the user's to change
//! (their email, which identifies the account for sign-in, and their role) stay
//! read-only here and remain Site-Admin-managed in
//! [`crate::server_fns::users`].

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::server_fns::users::{AccountRole, User};
use crate::server_fns::volunteer_hours::VolunteerHours;
use crate::server_fns::volunteers::VolunteerApplication;

/// The personal contact details on a profile. Only ever populated for the
/// profile's owner and viewers with operations-admin permissions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileContact {
    pub email: String,
    pub phone: String,
    pub home_address: String,
}

/// The CRM person record linked to this account, when there is one.
///
/// A `users` row is an account; a `contacts` row is a person (ADR-0005). This
/// is the bridge, so a profile can say plainly which person record belongs to
/// this account. Only sent to viewers with operations-admin permissions, since
/// they are the only ones who can open the People workspace.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfilePersonRecord {
    pub contact_id: String,
    pub display_name: String,
    pub organization_name: String,
    pub types: Vec<String>,
    pub archived: bool,
}

/// A user's profile, resolved for one specific viewer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    pub role: AccountRole,
    /// Contact details, present only when the viewer is the profile's owner or
    /// has operations-admin permissions.
    #[serde(default)]
    pub contact: Option<ProfileContact>,
    /// Volunteer hours, present only for a volunteer-access account when the
    /// viewer owns this profile or has operations-admin permissions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volunteer_hours: Option<VolunteerHours>,
    /// This person's volunteer record: the agreement they accepted and where
    /// their application stands. Present only for the profile's owner and for
    /// viewers with operations-admin permissions, under the same rule as the
    /// contact block. `None` when they have never applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volunteer: Option<VolunteerApplication>,
    /// Whether this profile belongs to the caller (drives the edit affordances).
    #[serde(default)]
    pub is_self: bool,
    /// Whether the viewer works a case with this user — the reason a colleague's
    /// profile is visible. Always false on your own profile, and false for a
    /// viewer with operations-admin permissions who shares no case with them.
    #[serde(default)]
    pub shares_case: bool,
    /// The CRM person record for this account. Operations admins only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person: Option<ProfilePersonRecord>,
    /// Why this account was retired. Follows the same privacy rule as the
    /// contact block: the owner and operations admins only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deactivation: Option<crate::server_fns::users::AccountDeactivation>,
}

impl UserProfile {
    /// Narrow a stored [`User`] down to what one particular viewer is allowed
    /// to see.
    ///
    /// This is the one place the privacy rule lives, and it is deliberately a
    /// *constructor* rather than a filter applied afterwards: you cannot get a
    /// `UserProfile` at all without first answering whether this is the owner
    /// or a viewer with operations-admin permissions, so a caller cannot forget
    /// to strip the contact block.
    pub fn for_viewer(
        user: User,
        is_self: bool,
        has_operations_admin_permissions: bool,
        shares_case: bool,
        volunteer_hours: Option<VolunteerHours>,
        volunteer: Option<VolunteerApplication>,
        person: Option<ProfilePersonRecord>,
    ) -> Self {
        let User {
            id,
            first_name,
            last_name,
            email,
            phone,
            home_address,
            role,
            deactivation,
            ..
        } = user;
        Self {
            id,
            first_name,
            last_name,
            role,
            contact: (is_self || has_operations_admin_permissions).then_some(ProfileContact {
                email,
                phone,
                home_address,
            }),
            volunteer_hours,
            volunteer,
            is_self,
            shares_case,
            person,
            deactivation: (is_self || has_operations_admin_permissions)
                .then_some(deactivation)
                .flatten(),
        }
    }

    /// The profile's display name, falling back to the user id when unnamed.
    pub fn full_name(&self) -> String {
        let name = format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string();
        if name.is_empty() {
            self.id.clone()
        } else {
            name
        }
    }

    /// Up to two uppercase initials for the avatar bubble.
    pub fn initials(&self) -> String {
        crate::helpers::format::initials(&self.full_name())
    }

    /// The editable subset of this profile, for seeding the edit form. Only
    /// meaningful on your own profile, where the contact details are present.
    pub fn to_edit(&self) -> ProfileEdit {
        let contact = self.contact.clone().unwrap_or_default();
        ProfileEdit {
            first_name: self.first_name.clone(),
            last_name: self.last_name.clone(),
            phone: contact.phone,
            home_address: contact.home_address,
        }
    }
}

/// The fields a user may change on their own profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileEdit {
    pub first_name: String,
    pub last_name: String,
    pub phone: String,
    pub home_address: String,
}

/// Load a profile as seen by the signed-in caller.
///
/// Visible when the profile is the caller's own, the caller is an admin, or the
/// two share a case; otherwise the call fails rather than leaking that the
/// account exists in any more detail. Contact details ride along only for the
/// owner and for admins.
#[server(prefix = "/api")]
pub async fn load_profile(user_id: String) -> Result<UserProfile, ServerFnError> {
    use crate::server::db::{contacts, users, volunteer_hours, volunteers};
    use crate::server::permissions::{has_volunteer_access, require_user};

    let viewer = require_user().await?;
    let user_id = user_id.trim().to_string();
    if user_id.is_empty() {
        return Err(ServerFnError::new("No profile was requested."));
    }

    const UNAVAILABLE: &str =
        "This profile isn't available. You can only view the profiles of people you share a case with.";

    let record = users::get(&user_id)
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| ServerFnError::new(UNAVAILABLE))?;

    let is_self = viewer.id == record.id;
    let shares_case = if is_self {
        false
    } else {
        users::shares_case(&viewer.id, &record.id)
            .await
            .map_err(ServerFnError::new)?
    };
    let has_operations_admin_permissions = viewer.role.has_operations_admin_permissions();
    let has_information_management_access = viewer.has_information_management_access();
    if !is_self && !shares_case && !has_operations_admin_permissions {
        return Err(ServerFnError::new(UNAVAILABLE));
    }

    let hours = if (is_self || has_operations_admin_permissions) && has_volunteer_access(&record) {
        Some(
            volunteer_hours::list_for_user(&record.id)
                .await
                .map_err(ServerFnError::new)?,
        )
    } else {
        None
    };

    // Same rule as the contact block: the owner and operations admins only.
    let volunteer = if is_self || has_operations_admin_permissions {
        volunteers::get(&record.id)
            .await
            .map_err(ServerFnError::new)?
    } else {
        None
    };

    // The reverse account link remains an account-administration detail.
    let person = if has_operations_admin_permissions && has_information_management_access {
        contacts::for_user(&record.id)
            .await
            .map_err(ServerFnError::new)?
            .map(|c| ProfilePersonRecord {
                display_name: c.display_name(),
                organization_name: c.organization_name.clone(),
                types: c.types.iter().map(|t| t.label().to_string()).collect(),
                archived: c.archived,
                contact_id: c.id,
            })
    } else {
        None
    };

    Ok(UserProfile::for_viewer(
        record,
        is_self,
        has_operations_admin_permissions,
        shares_case,
        hours,
        volunteer,
        person,
    ))
}

/// Save and return the caller's normalized editable profile fields. Always
/// scoped to the signed-in user: there is no way to edit somebody else's
/// profile here, and unrelated profile data is neither read nor returned.
#[server(prefix = "/api")]
pub async fn save_my_profile(edit: ProfileEdit) -> Result<ProfileEdit, ServerFnError> {
    use crate::server::db::users;
    use crate::server::permissions::require_user;

    let user = require_user().await?;
    let edit = ProfileEdit {
        first_name: edit.first_name.trim().to_string(),
        last_name: edit.last_name.trim().to_string(),
        phone: edit.phone.trim().to_string(),
        home_address: edit.home_address.trim().to_string(),
    };
    if edit.first_name.is_empty() || edit.last_name.is_empty() {
        return Err(ServerFnError::new("First and last name are required."));
    }

    users::update_profile(&user.id, &edit, &user.full_name())
        .await
        .map_err(ServerFnError::new)?;

    Ok(edit)
}
