//! The public volunteer signup: applying without an account, the admin decision
//! on it, and the setup link that turns an approved application into a real
//! account.
//!
//! Distinct from [`crate::server_fns::volunteers`], which is about people who
//! already have an account. Here no `users` row exists until
//! [`complete_volunteer_setup`] runs, so nothing an applicant submits can be
//! signed in with, and a declined application leaves no account behind.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::helpers::volunteer_details::{VolunteerDetails, VolunteerDetailsView};

/// One volunteer application from somebody with no account yet, as the admin
/// queue shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolunteerApplicant {
    pub id: String,
    pub first_name: String,
    pub last_name: String,
    /// The address they applied with; every notification goes here.
    pub email: String,
    pub agreement_version: String,
    /// Pre-formatted for display; only ever shown, never compared.
    pub agreed_at: String,
    /// `pending`, `approved`, `denied` or `completed`.
    pub status: String,
    /// The official address an approving admin issued, empty when they keep the
    /// one they applied with.
    pub assigned_email: String,
    pub decided_by_name: String,
    pub decision_note: String,
    pub decided_at: String,
    /// When an approved applicant's setup link stops working, for display.
    #[serde(default)]
    pub setup_expires_at: String,
    #[serde(default)]
    pub setup_expired: bool,
    /// What they told us about themselves. Carries `has_ssn`, never the number.
    #[serde(default)]
    pub details: VolunteerDetailsView,
}

impl VolunteerApplicant {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
            .trim()
            .to_string()
    }

    /// Whether this application still awaits a decision. An approved one is
    /// waiting on the applicant instead.
    pub fn is_pending(&self) -> bool {
        self.status == "pending"
    }
}

/// What the account-setup page shows for a live setup link. Deliberately thin:
/// it is served to anyone holding the link, so it carries no details beyond who
/// the link is for and which address will sign them in.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolunteerSetup {
    pub first_name: String,
    pub last_name: String,
    /// The address this account will sign in with.
    pub sign_in_email: String,
    /// The address they applied with, which the link was emailed to.
    pub original_email: String,
    /// Whether an admin issued them a different address, so the page can say so
    /// plainly rather than leaving them to notice.
    pub email_changed: bool,
}

/// File a volunteer application from the public signup form. Creates no account:
/// the signed agreement is staged until an admin decides on it.
#[server(prefix = "/api")]
pub async fn apply_as_volunteer(
    first_name: String,
    last_name: String,
    email: String,
    agreement_version: String,
    details: VolunteerDetails,
) -> Result<(), ServerFnError> {
    use crate::server::db::{throttle, users, volunteer_applicants};
    use crate::server_fns::auth::validate_account_lengths;

    let first_name = first_name.trim().to_string();
    let last_name = last_name.trim().to_string();
    let email = email.trim().to_lowercase();
    if first_name.is_empty() || last_name.is_empty() || email.is_empty() {
        return Err(ServerFnError::new(
            "Please give your first name, last name, and email address.",
        ));
    }
    if !email.contains('@') {
        return Err(ServerFnError::new("Please enter a valid email address."));
    }
    validate_account_lengths(&first_name, &last_name, &email)?;

    // An old tab holding a stale version never saw the wording it claims to
    // accept, so make it re-read rather than record consent it cannot evidence.
    if !crate::helpers::volunteer_terms::is_current(agreement_version.trim()) {
        return Err(ServerFnError::new(
            "Please read and accept the current volunteer agreement.",
        ));
    }

    // Normalize first so what is validated is exactly what gets stored. Signing
    // requires the SSN outright, plus the consent and typed signature.
    let details = details.normalized();
    details.validate_with(false).map_err(ServerFnError::new)?;
    details.validate_signature().map_err(ServerFnError::new)?;

    // Rate-limited on the same budget as a client signup: this endpoint also
    // sends mail on somebody else's say-so.
    if let Some(seconds) = throttle::seconds_locked(throttle::Action::Register, &email)
        .await
        .map_err(ServerFnError::new)?
    {
        let minutes = throttle::minutes_remaining(seconds);
        return Err(ServerFnError::new(format!(
            "Too many sign-up attempts. Please try again in about {minutes} minute{}.",
            if minutes == 1 { "" } else { "s" }
        )));
    }

    if users::email_exists(&email)
        .await
        .map_err(ServerFnError::new)?
    {
        return Err(ServerFnError::new(
            "An account with that email already exists. Please sign in instead.",
        ));
    }
    if volunteer_applicants::live_email_exists(&email)
        .await
        .map_err(ServerFnError::new)?
    {
        return Err(ServerFnError::new(
            "We already have a volunteer application for that email address.",
        ));
    }

    let _ = throttle::record_failure(throttle::Action::Register, &email).await;

    volunteer_applicants::create(
        &first_name,
        &last_name,
        &email,
        agreement_version.trim(),
        &details,
    )
    .await
    .map_err(|error| ServerFnError::new(error.to_string()))?;

    crate::server::notifications::notify_volunteer_application_filed(
        format!("{first_name} {last_name}").trim().to_string(),
        email,
    );
    Ok(())
}

/// Every volunteer application from someone with no account: those awaiting a
/// decision, and those approved but not yet completed. Operations admins may
/// inspect the queue; only site admins may decide.
#[server(prefix = "/api")]
pub async fn list_volunteer_applicants() -> Result<Vec<VolunteerApplicant>, ServerFnError> {
    use crate::server::db::volunteer_applicants;
    use crate::server::permissions::{require_operations_admin, require_user};

    let actor = require_user().await?;
    require_operations_admin(&actor)?;
    volunteer_applicants::list_open()
        .await
        .map_err(ServerFnError::new)
}

/// Approve or decline a volunteer application from someone with no account.
/// Site admins only, because approving leads to a Volunteer account.
///
/// On approval the admin may set the official email address the new account will
/// sign in with. It must be typed twice. Either way the applicant is emailed at
/// the address they applied with; an approval carries the setup link, which is
/// the only thing that can create the account. With a new address, the admin may
/// add its temporary Microsoft password; it goes in that email and is never saved.
#[server(prefix = "/api")]
pub async fn decide_volunteer_applicant(
    applicant_id: String,
    approve: bool,
    note: String,
    new_email: String,
    confirm_new_email: String,
    #[server(default)] temporary_password: String,
) -> Result<(), ServerFnError> {
    use crate::server::auth::generate_token;
    use crate::server::db::volunteer_applicants;
    use crate::server::permissions::{require_site_admin, require_user};
    use crate::server_fns::auth::validate_confirmed_email;

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    let applicant_id = applicant_id.trim().to_string();
    if applicant_id.is_empty() {
        return Err(ServerFnError::new("No application was chosen."));
    }
    if note.chars().count() > 1_000 {
        return Err(ServerFnError::new(
            "Notes must be 1,000 characters or fewer.",
        ));
    }
    let note = note.trim().to_string();

    let applicant = volunteer_applicants::get(&applicant_id)
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| ServerFnError::new("No volunteer application was found."))?;

    // Only an approval may carry an address change, and only a non-empty one
    // asks for it at all.
    let assigned_email = if approve && !new_email.trim().is_empty() {
        let address = validate_confirmed_email(&new_email, &confirm_new_email)?;
        // Assigning the address they already applied with is a no-op, not a
        // change to announce.
        (!address.eq_ignore_ascii_case(&applicant.email)).then_some(address)
    } else {
        None
    };
    // Only meaningful alongside a new mailbox.
    let temporary_password = temporary_password.trim().to_string();
    if temporary_password.chars().count() > 256 {
        return Err(ServerFnError::new(
            "The temporary password must be 256 characters or fewer.",
        ));
    }
    let temporary_password =
        (assigned_email.is_some() && !temporary_password.is_empty()).then_some(temporary_password);

    let setup_token = generate_token();
    volunteer_applicants::decide(
        &applicant_id,
        approve,
        &actor.id,
        &actor.full_name(),
        &note,
        assigned_email.as_deref(),
        &setup_token,
    )
    .await
    .map_err(|error| ServerFnError::new(error.to_string()))?;

    let setup_url = approve.then(|| setup_url(&setup_token));
    crate::server::notifications::notify_volunteer_applicant_decision(
        applicant.first_name,
        applicant.email,
        approve,
        note,
        setup_url,
        assigned_email,
        temporary_password,
    );
    Ok(())
}

/// A setup link a site admin just renewed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenewedSetupLink {
    /// Only present when the admin asked to pass the link on themselves.
    pub link: Option<String>,
    pub expires_at: String,
}

/// Give an approved applicant a fresh setup link, for when the first one expired
/// or never arrived. With `send_email` it goes out like the approval did;
/// otherwise the link is returned for a site admin to pass on another way.
#[server(prefix = "/api")]
pub async fn renew_volunteer_setup_link(
    applicant_id: String,
    send_email: bool,
) -> Result<RenewedSetupLink, ServerFnError> {
    use crate::server::auth::generate_token;
    use crate::server::db::volunteer_applicants;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    let applicant = volunteer_applicants::get(applicant_id.trim())
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| ServerFnError::new("No volunteer application was found."))?;

    // Checked before renewing, so the old link still works if nothing can go out.
    if send_email && !crate::server::notifications::email_enabled() {
        return Err(ServerFnError::new(
            "Email isn't set up on this server \u{2014} use Get setup link instead.",
        ));
    }

    let setup_token = generate_token();
    let expires_at = volunteer_applicants::renew_setup(
        &applicant.id,
        &setup_token,
        &actor.id,
        &actor.full_name(),
        send_email,
    )
    .await
    .map_err(|error| ServerFnError::new(error.to_string()))?;
    tracing::info!(
        actor = %actor.id,
        applicant = %applicant.id,
        emailed = send_email,
        "renewed volunteer setup link"
    );

    let url = setup_url(&setup_token);
    if !send_email {
        return Ok(RenewedSetupLink {
            link: Some(url),
            expires_at,
        });
    }
    crate::server::notifications::notify_volunteer_applicant_decision(
        applicant.first_name,
        applicant.email,
        true,
        applicant.decision_note,
        Some(url),
        (!applicant.assigned_email.is_empty()).then_some(applicant.assigned_email),
        None,
    );
    Ok(RenewedSetupLink {
        link: None,
        expires_at,
    })
}

/// Withdraw an approval nobody has used yet, such as a duplicate signup. Site
/// admins only. The applicant is not emailed.
#[server(prefix = "/api")]
pub async fn withdraw_volunteer_applicant(applicant_id: String) -> Result<(), ServerFnError> {
    use crate::server::db::volunteer_applicants;
    use crate::server::permissions::{require_site_admin, require_user};

    let actor = require_user().await?;
    require_site_admin(&actor)?;
    volunteer_applicants::withdraw(applicant_id.trim(), &actor.id, &actor.full_name())
        .await
        .map_err(|error| ServerFnError::new(error.to_string()))
}

#[cfg(feature = "ssr")]
fn setup_url(setup_token: &str) -> String {
    let base = crate::server::config::Brand::from_env().app_url;
    let base = if base.is_empty() {
        "https://app.example.org".to_string()
    } else {
        base
    };
    format!("{base}/volunteer-setup?token={setup_token}")
}

/// What the setup page shows for a setup link: who it is for and which address
/// will sign them in. Public, because the holder of the link has no account to
/// authenticate with yet; the token is the credential.
#[server(prefix = "/api")]
pub async fn volunteer_setup_details(token: String) -> Result<VolunteerSetup, ServerFnError> {
    use crate::server::db::volunteer_applicants;

    volunteer_applicants::setup_for_token(token.trim())
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| {
            ServerFnError::new(
                "This setup link is invalid or has expired. Please contact an administrator.",
            )
        })
}

/// Redeem a setup link: choose a password, and the approved application becomes
/// a real Volunteer account, signed in immediately.
#[server(prefix = "/api")]
pub async fn complete_volunteer_setup(
    token: String,
    password: String,
    password_confirmation: String,
) -> Result<crate::server_fns::users::User, ServerFnError> {
    use crate::server::auth::{build_session_cookie, hash_password};
    use crate::server::db::{sessions, users, volunteer_applicants};
    use crate::server_fns::auth::{append_session_cookie, validate_password};

    if password != password_confirmation {
        return Err(ServerFnError::new("The passwords do not match."));
    }
    validate_password(&password)?;

    let password_hash = hash_password(&password).map_err(ServerFnError::new)?;
    let (user_id, _email) = volunteer_applicants::complete(token.trim(), &password_hash)
        .await
        .map_err(|error| ServerFnError::new(error.to_string()))?;

    let user = users::get(&user_id)
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| ServerFnError::new("User disappeared after insert."))?;

    let raw = sessions::create(&user_id)
        .await
        .map_err(ServerFnError::new)?;
    append_session_cookie(build_session_cookie(raw))?;
    Ok(user)
}
