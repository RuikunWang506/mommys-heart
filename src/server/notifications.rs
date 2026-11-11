//! Best-effort email notifications for case activity (SSR only).
//!
//! The case-mutating server functions in [`crate::server_fns`] call the helpers
//! here *after* a change succeeds. Each helper resolves the recipients, filters
//! them by their own notification settings, and sends the emails on a background
//! task via [`crate::server::email`]. Nothing here ever blocks or fails the
//! originating request — failures are logged and swallowed, mirroring
//! [`crate::server::rag::start_background_ingest`] and the audit retention task.
//!
//! When ACS Email is not configured (see [`EmailConfig::is_configured`]), every
//! helper is a silent no-op unless dry-run mode is enabled.

use crate::helpers::visibility::Visibility;
use crate::server::config::{Brand, EmailConfig};
use crate::server::db::audit;
use crate::server::db::cases;
use crate::server::db::settings::{self, Recipient};
use crate::server::email::templates::{self, RenderedEmail};
use crate::server::email::{
    send_email, EmailKind, EmailMessage, EmailRecipient, EmailRecipients,
    MAX_RECIPIENTS_PER_MESSAGE,
};
use crate::server_fns::admin_requests::AdminRequest;
use crate::server_fns::settings::NotificationKind;

/// The most events one digest email will list. Any beyond this are reported as
/// a count and remain visible in the Admin activity feed.
const MAX_DIGEST_EVENTS: i64 = 200;

/// The most delivery failures one digest email will list. Any beyond this are
/// reported as a count and remain visible in the admin email-failure log.
const MAX_DIGEST_FAILURES: i64 = 50;

/// How often the admin-activity digest is mailed — once a day. A constant
/// rather than configuration, like the retention schedules in
/// [`crate::server::db::audit`]: it is a product decision, not a deployment knob.
const ADMIN_ACTIVITY_DIGEST_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(24 * 60 * 60);

/// The active email config, or `None` when neither delivery nor dry-run logging
/// is configured.
fn configured_email() -> Option<EmailConfig> {
    let cfg = EmailConfig::from_env();
    (cfg.dry_run || cfg.is_configured()).then_some(cfg)
}

/// Whether notifications would be sent (or dry-run logged) at all.
pub fn email_enabled() -> bool {
    configured_email().is_some()
}

/// Who a case notification should reach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Audience {
    Everyone,
    StaffOnly,
}

/// Who to notify about a change to case information of a given visibility.
pub fn audience_for(visibility: Visibility) -> Audience {
    if visibility.is_restricted() {
        Audience::StaffOnly
    } else {
        Audience::Everyone
    }
}

/// Fire a best-effort case notification to everyone assigned to the case (with
/// `view_case`) except the actor, honoring each recipient's settings.
///
/// `detail` is a human sentence describing what the actor did, e.g. `changed the
/// status to "Closed"`; it is combined with `actor_name` and the resolved case
/// name to build the email. Returns immediately.
pub fn notify_case(
    case_id: String,
    actor_id: String,
    actor_name: String,
    kind: NotificationKind,
    detail: String,
    audience: Audience,
) {
    if kind == NotificationKind::NewMessage {
        notify_secure_message(case_id, actor_id, audience);
        return;
    }
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let staff_only = audience == Audience::StaffOnly;
        let mut recipients =
            match settings::recipients_for_case(&case_id, &actor_id, staff_only).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("notify_case: recipient lookup failed for {case_id}: {e}");
                    return;
                }
            };
        recipients.retain(|recipient| recipient.settings.wants(kind));
        if recipients.is_empty() {
            return;
        }
        let case = case_name(&case_id).await;
        let email = templates::case_event(&Brand::from_env(), kind, &case, &actor_name, &detail);
        dispatch(&cfg, recipients, &email, "Case notification").await;
    });
}

/// Fire a best-effort content-free secure-message notice.
pub fn notify_secure_message(case_id: String, actor_id: String, audience: Audience) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let staff_only = audience == Audience::StaffOnly;
        let mut recipients = match settings::recipients_for_case(&case_id, &actor_id, staff_only)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("notify_secure_message: recipient lookup failed for {case_id}: {e}");
                return;
            }
        };
        recipients.retain(|recipient| recipient.settings.wants(NotificationKind::NewMessage));
        if recipients.is_empty() {
            return;
        }
        let email = templates::secure_message_notice(&Brand::from_env());
        dispatch(&cfg, recipients, &email, "Secure message notification").await;
    });
}

/// Fire a best-effort "your case was accepted / not accepted" notification.
pub fn notify_case_decision(
    case_id: String,
    actor_id: String,
    actor_name: String,
    accepted: bool,
    reason: String,
) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let mut recipients = match settings::recipients_for_case(&case_id, &actor_id, false).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("notify_case_decision: recipient lookup failed for {case_id}: {e}");
                return;
            }
        };
        recipients.retain(|recipient| recipient.settings.wants(NotificationKind::CaseData));
        if recipients.is_empty() {
            return;
        }
        let case = case_name(&case_id).await;
        let email =
            templates::case_decision(&Brand::from_env(), &case, &actor_name, accepted, &reason);
        dispatch(&cfg, recipients, &email, "Case decision").await;
    });
}

/// Fire a best-effort "you were assigned to a case" notification to the newly
/// assigned user (the recipient is that user, not the case's other members).
pub fn notify_assignment(user_id: String, actor_name: String, case_id: String) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let recipient = match settings::recipient_for_user(&user_id).await {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("notify_assignment: lookup failed for {user_id}: {e}");
                return;
            }
        };
        if !recipient
            .settings
            .wants(NotificationKind::AccountPermissionsChanged)
        {
            return;
        }
        let case = case_name(&case_id).await;
        let email = templates::assignment(&Brand::from_env(), &case, &actor_name);
        dispatch(&cfg, vec![recipient], &email, "Case notification").await;
    });
}

/// Notify a user that their account-level role or information permission changed.
pub fn notify_account_permissions_changed(user_id: String, actor_name: String, change: String) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let recipient = match settings::recipient_for_user(&user_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!("account-permission recipient lookup failed for {user_id}: {error}");
                return;
            }
        };
        if !recipient
            .settings
            .wants(NotificationKind::AccountPermissionsChanged)
        {
            return;
        }
        let email =
            templates::account_permissions_changed(&Brand::from_env(), &actor_name, &change);
        dispatch(
            &cfg,
            vec![recipient],
            &email,
            "Account permission notification",
        )
        .await;
    });
}

/// Notify all site admins that an operations-admin request is waiting for
/// review, honouring each administrator's notification settings.
pub fn notify_admin_request_filed(request: AdminRequest) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let mut recipients = match settings::recipients_for_site_admins().await {
            Ok(recipients) => recipients,
            Err(error) => {
                tracing::warn!("admin request recipient lookup failed: {error}");
                return;
            }
        };
        recipients.retain(|recipient| recipient.settings.wants(NotificationKind::AdminRequests));
        if recipients.is_empty() {
            return;
        }
        let email = templates::admin_request_filed(&Brand::from_env(), &request);
        dispatch(&cfg, recipients, &email, "Admin request notification").await;
    });
}

/// Notify the requester when a site admin approves or denies an administrative
/// request. Affected users receive the notification for the resulting domain
/// event, such as [`notify_assignment`] or [`notify_account_permissions_changed`], instead.
pub fn notify_admin_request_decided(request: AdminRequest) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let recipients = match settings::recipient_for_user(&request.requested_by_id).await {
            Ok(Some(recipient)) if recipient.settings.wants(NotificationKind::AdminRequests) => {
                vec![recipient]
            }
            Ok(None) => Vec::new(),
            Ok(Some(_)) => Vec::new(),
            Err(error) => {
                tracing::warn!(
                    "admin request decision recipient lookup failed for {}: {error}",
                    request.requested_by_id
                );
                return;
            }
        };
        let email = templates::admin_request_decided(&Brand::from_env(), &request);
        dispatch(&cfg, recipients, &email, "Admin request notification").await;
    });
}

/// Notify site admins that a volunteer application is waiting for review. Only
/// site admins may decide one, so this matches [`notify_admin_request_filed`]
/// rather than notifying every administrator.
pub fn notify_volunteer_application_filed(applicant_name: String, applicant_email: String) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let mut recipients = match settings::recipients_for_site_admins().await {
            Ok(recipients) => recipients,
            Err(error) => {
                tracing::warn!("volunteer application recipient lookup failed: {error}");
                return;
            }
        };
        recipients.retain(|recipient| recipient.settings.wants(NotificationKind::AdminRequests));
        if recipients.is_empty() {
            return;
        }
        let email = templates::volunteer_application_filed(
            &Brand::from_env(),
            &applicant_name,
            &applicant_email,
        );
        dispatch(&cfg, recipients, &email, "Volunteer application").await;
    });
}

/// Notify an applicant that their volunteer application was approved or declined.
/// Approval follows the account-permission preference; decline remains required
/// because it is the applicant's only notice.
pub fn notify_volunteer_decision(
    user_id: String,
    approved: bool,
    decision_note: String,
    previous_email: Option<String>,
) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let recipient = match settings::recipient_for_user(&user_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!("volunteer decision recipient lookup failed for {user_id}: {error}");
                return;
            }
        };
        if approved
            && !recipient
                .settings
                .wants(NotificationKind::AccountPermissionsChanged)
        {
            return;
        }
        // The lookup runs after commit, so this is already the new address.
        let new_email = previous_email
            .as_ref()
            .map(|_| recipient.email.clone())
            .filter(|_| approved);
        let email = templates::volunteer_application_decided(
            &Brand::from_env(),
            approved,
            &decision_note,
            new_email.as_deref(),
        );
        // Copy the old address too, so someone who cannot yet read the new
        // mailbox still learns which address now signs them in.
        let mut recipients = vec![recipient.clone()];
        if let Some(previous) = previous_email.filter(|_| approved) {
            recipients.push(Recipient {
                email: previous,
                ..recipient
            });
        }
        for recipient in recipients {
            dispatch(&cfg, vec![recipient], &email, "Volunteer application").await;
        }
    });
}

/// Notify a volunteer applicant who has no account yet that their application
/// was decided.
///
/// Unlike [`notify_volunteer_decision`], there is no user to look preferences up
/// for: the recipient is the raw address they applied with, and both outcomes
/// are required notices — approval because it carries the only link that can
/// complete the account, decline because it is their only word on the matter.
/// The approval always goes to the *original* address even when an admin issued
/// a new one, since that is the only mailbox they are known to hold.
pub fn notify_volunteer_applicant_decision(
    first_name: String,
    email: String,
    approved: bool,
    decision_note: String,
    setup_url: Option<String>,
    new_email: Option<String>,
    temporary_password: Option<String>,
) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let brand = Brand::from_env();
        let message = if approved {
            let Some(setup_url) = setup_url else {
                tracing::warn!("volunteer applicant approved with no setup link for {email}");
                return;
            };
            templates::volunteer_applicant_approved(
                &brand,
                &first_name,
                &setup_url,
                &decision_note,
                new_email.as_deref(),
                temporary_password.as_deref(),
            )
        } else {
            templates::volunteer_applicant_declined(&brand, &decision_note)
        };
        // No account means no stored preferences; every category is on by
        // default, and these are required notices in any case.
        let recipient = Recipient {
            email,
            name: first_name,
            settings: crate::server_fns::settings::NotificationSettings::all_on(),
        };
        dispatch(&cfg, vec![recipient], &message, "Volunteer application").await;
    });
}

/// Notify a user that an administrator changed their primary email address.
/// Sent to both the new and old addresses: it changes how they sign in, so it
/// is a required account notice rather than an optional one.
pub fn notify_email_changed(user_id: String, actor_name: String, previous_email: String) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let recipient = match settings::recipient_for_user(&user_id).await {
            Ok(Some(recipient)) => recipient,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!("email-change recipient lookup failed for {user_id}: {error}");
                return;
            }
        };
        let email = templates::account_permissions_changed(
            &Brand::from_env(),
            &actor_name,
            &format!("changed your sign-in email address to {}", recipient.email),
        );
        let old = Recipient {
            email: previous_email,
            ..recipient.clone()
        };
        for recipient in [recipient, old] {
            dispatch(&cfg, vec![recipient], &email, "Email address change").await;
        }
    });
}

/// Notify site and operations administrators after a client verifies their
/// email and their new account and case have been committed.
pub fn notify_case_signup(client_name: String, client_email: String, case_name: String) {
    let Some(cfg) = configured_email() else {
        return;
    };
    tokio::spawn(async move {
        let mut recipients = match settings::recipients_for_admins().await {
            Ok(recipients) => recipients,
            Err(error) => {
                tracing::warn!("case signup recipient lookup failed: {error}");
                return;
            }
        };
        recipients.retain(|recipient| recipient.settings.wants(NotificationKind::AdminRequests));
        if recipients.is_empty() {
            return;
        }
        let email =
            templates::case_signup(&Brand::from_env(), &client_name, &client_email, &case_name);
        dispatch(&cfg, recipients, &email, "Case signup notification").await;
    });
}

/// Spawn the background task that mails the admin activity digest, once a day
/// ([`ADMIN_ACTIVITY_DIGEST_INTERVAL`]).
///
/// The digest reports what the audit logs recorded since the last one, tracked
/// by a watermark rather than by marking the audit rows themselves — nothing
/// here ever writes to an append-only log.
///
/// When email is neither configured nor in dry-run mode the watermark is left
/// alone: activity keeps accumulating and stays visible in the feed, so turning
/// email on later loses nothing. A send that fails is recorded in
/// `email_failures`.
pub fn start_admin_activity_digest_task() {
    tokio::spawn(async {
        loop {
            // Wait first: a restart should not fire a digest of whatever was
            // pending, and an app that restarts often would otherwise spam.
            tokio::time::sleep(ADMIN_ACTIVITY_DIGEST_INTERVAL).await;
            send_admin_activity_digest().await;
        }
    });
}

/// One digest pass. Separated from the loop so the logic reads on its own.
async fn send_admin_activity_digest() {
    let Some(cfg) = configured_email() else {
        return;
    };

    let mut recipients = match settings::recipients_for_admins().await {
        Ok(recipients) => recipients,
        Err(error) => {
            tracing::warn!("admin activity digest: recipient lookup failed: {error}");
            return;
        }
    };
    recipients.retain(|recipient| recipient.settings.wants(NotificationKind::AdminActivity));
    if recipients.is_empty() {
        // Nobody wants it, so leave the watermark for whoever opts in later.
        return;
    }

    let watermark = match audit::activity_watermark().await {
        Ok(watermark) => watermark,
        Err(error) => {
            tracing::warn!("admin activity digest: watermark read failed: {error}");
            return;
        }
    };
    let (events, overflow, mut next) =
        match audit::activity_since(watermark, MAX_DIGEST_EVENTS).await {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!("admin activity digest: activity lookup failed: {error}");
                return;
            }
        };

    // Delivery failures come from their own log. A lookup error costs the
    // failure section, not the whole digest.
    let (failures, failures_extra) = match crate::server::db::email_failures::since(
        watermark.last_failure_seq,
        MAX_DIGEST_FAILURES,
    )
    .await
    {
        Ok((failures, extra, next_failure_seq)) => {
            next.last_failure_seq = next_failure_seq;
            (failures, extra)
        }
        Err(error) => {
            tracing::warn!("admin activity digest: failure lookup failed: {error}");
            (Vec::new(), 0)
        }
    };
    if events.is_empty() && failures.is_empty() {
        // Still advance: the window may have held only unclassified rows, and
        // rescanning them every day would grow without bound.
        if let Err(error) = audit::set_activity_watermark(next).await {
            tracing::warn!("admin activity digest: watermark update failed: {error}");
        }
        return;
    }

    let email = templates::admin_activity_digest(
        &Brand::from_env(),
        &events,
        overflow,
        &failures,
        failures_extra,
    );
    dispatch(&cfg, recipients, &email, "Admin activity digest").await;
    if let Err(error) = audit::set_activity_watermark(next).await {
        tracing::warn!("admin activity digest: watermark update failed: {error}");
    }
}

/// Send one direct message or BCC chunks when the rendered content is shared.
async fn dispatch(
    cfg: &EmailConfig,
    recipients: Vec<Recipient>,
    email: &RenderedEmail,
    context: &str,
) {
    for chunk in recipients.chunks(MAX_RECIPIENTS_PER_MESSAGE) {
        if cfg.dry_run {
            let addresses = chunk
                .iter()
                .map(|recipient| recipient.email.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                "[email dry-run] would send \"{}\" to {} ({} recipient(s); \
                 set EMAIL_DRY_RUN=false to send)",
                email.subject,
                addresses,
                chunk.len()
            );
            continue;
        }

        let recipients = if let [recipient] = chunk {
            EmailRecipients::To(email_recipient(recipient))
        } else {
            EmailRecipients::Bcc(chunk.iter().map(email_recipient).collect())
        };
        let message = EmailMessage {
            kind: EmailKind::Standard,
            recipients,
            subject: email.subject.clone(),
            html: email.html.clone(),
            plain_text: email.plain_text.clone(),
        };
        if let Err(error) = send_email(cfg, &message).await {
            tracing::warn!(
                "notification email batch for {} recipient(s) failed: {error}",
                chunk.len()
            );
            for recipient in chunk {
                crate::server::db::email_failures::record(
                    &recipient.email,
                    &email.subject,
                    context,
                    &error,
                )
                .await;
            }
        }
    }
}

fn email_recipient(recipient: &Recipient) -> EmailRecipient {
    EmailRecipient {
        address: recipient.email.clone(),
        name: recipient.name.clone(),
    }
}

/// The case's display name, falling back to its id if the row is missing.
async fn case_name(case_id: &str) -> String {
    cases::name(case_id)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| case_id.to_string())
}
