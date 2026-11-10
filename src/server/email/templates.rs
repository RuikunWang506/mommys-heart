//! Branded HTML email templates for app notifications (SSR only).
//!
//! Every notification email is built here so the look — colors, header, footer,
//! button, spacing — is defined once and shared by all message types. Callers in
//! [`crate::server::notifications`] hand us the *event* (who did what, on which
//! case) and get back a [`RenderedEmail`] with the subject line plus matching
//! HTML and plain-text bodies, ready for [`crate::server::email::send_email`].
//!
//! The HTML is deliberately old-school — table-based layout with inline styles —
//! because that is what renders reliably across email clients (Outlook, Gmail,
//! Apple Mail). Shared building blocks ([`layout`], [`button`], the
//! [`palette`](super::palette) colors sourced from the app's Tailwind theme, and
//! the per-kind [`Theme`]) keep the individual templates short.
//!
//! To *see* these without sending anything, use the offline preview tooling
//! ([`samples`], wired to the `preview-emails` CLI command) which renders every
//! template with placeholder data to standalone HTML files.

use crate::server::config::Brand;
use crate::server::email::palette;
use crate::server_fns::admin_activity::{AdminActivityCategory, AdminActivityEvent};
use crate::server_fns::admin_requests::{AdminRequest, AdminRequestStatus};
use crate::server_fns::email_failures::EmailFailure;
use crate::server_fns::settings::NotificationKind;

/// Where an issued organizational address opens its mailbox. The approval email
/// points new volunteers here; their Microsoft password comes from an admin.
const OUTLOOK_SIGN_IN_URL: &str = "https://outlook.office365.com";

/// A fully rendered email: the subject line and both body representations ACS
/// expects (HTML plus a plain-text fallback for clients that prefer it).
#[derive(Clone, Debug)]
pub struct RenderedEmail {
    pub subject: String,
    pub html: String,
    pub plain_text: String,
}

/// Per-notification-kind theming: the accent color for the callout rule and a
/// small emoji glyph, so each kind is visually distinct while sharing the layout.
/// Accents are Tailwind status tokens resolved via [`palette`].
struct Theme {
    accent: &'static str,
    emoji: &'static str,
}

fn theme(kind: NotificationKind) -> Theme {
    match kind {
        NotificationKind::NewMessage => Theme {
            accent: palette::color("sky-300"),
            emoji: "\u{1F4AC}",
        }, // 💬
        NotificationKind::CaseData => Theme {
            accent: palette::color("amber-300"),
            emoji: "\u{270F}\u{FE0F}",
        }, // ✏️
        NotificationKind::NoteAdded => Theme {
            accent: palette::color("emerald-300"),
            emoji: "\u{1F4DD}",
        }, // 📝
        NotificationKind::EvidenceChanged => Theme {
            accent: palette::color("rose-400"),
            emoji: "\u{1F4CE}",
        }, // 📎
        NotificationKind::AccountPermissionsChanged => Theme {
            accent: palette::color("primary-500"),
            emoji: "\u{1F511}",
        }, // 🔑
        NotificationKind::AdminRequests => Theme {
            accent: palette::color("amber-300"),
            emoji: "\u{1F4CB}",
        }, // 📋
        NotificationKind::AdminActivity => Theme {
            // The palette only carries the tokens `style/tailwind.css` defines;
            // sky-200 is the darkest blue in it and is unused by other kinds.
            accent: palette::color("sky-200"),
            emoji: "\u{1F4E1}",
        }, // 📡
    }
}

/// Build the email for a case-activity event: `actor_name` did `detail` on the
/// case, categorized as `kind`. Used for status/name/owner/property changes,
/// notes, evidence, and new chat messages.
pub fn case_event(
    brand: &Brand,
    kind: NotificationKind,
    case_name: &str,
    actor_name: &str,
    detail: &str,
) -> RenderedEmail {
    let theme = theme(kind);
    let actor = actor_or_default(actor_name);
    let subject = format!("[{}] {}: {}", brand.name, case_name, kind.label());

    let callout = format!(
        "<strong>{actor}</strong> {detail} on the case <strong>{case}</strong>.",
        actor = escape(actor),
        detail = escape(detail),
        case = escape(case_name),
    );
    let cta_href = cta_url(brand, "/cases");
    let footer = notification_footer(brand);
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &format!("{actor} {detail} on \u{201C}{case_name}\u{201D}."),
            eyebrow: kind.label(),
            heading: &format!("Update on {}", case_name),
            callout_html: &callout,
            cta: cta_href.as_deref().map(|u| (u, "Open the case")),
            body_note: &format!("Sign in to {} to see the full details.", brand.name),
            footer_html: &footer,
        },
    );
    let plain = plain(
        brand,
        &format!("{actor} {detail} on the case \u{201C}{case_name}\u{201D}."),
        "/cases",
    );

    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// Build the content-free notice for secure case messaging.
pub fn secure_message_notice(brand: &Brand) -> RenderedEmail {
    let theme = theme(NotificationKind::NewMessage);
    let subject = format!("[{}] New secure message", brand.name);
    let callout = "A new secure message is waiting for you in the application.";
    let cta_href = cta_url(brand, "/inbox");
    let footer = notification_footer(brand);
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "A new secure message is waiting for you.",
            eyebrow: "Secure message",
            heading: "New secure message",
            callout_html: callout,
            cta: cta_href.as_deref().map(|u| (u, "Sign in")),
            body_note: &format!("Sign in to {} to read and reply securely.", brand.name),
            footer_html: &footer,
        },
    );
    let plain = plain(
        brand,
        "A new secure message is waiting for you in the application.",
        "/inbox",
    );
    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// Build the email telling a client their case was accepted or declined.
pub fn case_decision(
    brand: &Brand,
    case_name: &str,
    actor_name: &str,
    accepted: bool,
    reason: &str,
) -> RenderedEmail {
    let theme = theme(NotificationKind::CaseData);
    let actor = actor_or_default(actor_name);
    let outcome = if accepted { "accepted" } else { "not accepted" };
    let subject = format!("[{}] Your case was {}: {}", brand.name, outcome, case_name);

    let reason = reason.trim();
    // The reason gets its own sentence rather than being spliced into one.
    let detail_plain = if accepted {
        format!(
            "Your case \u{201C}{case_name}\u{201D} was accepted by {actor}. \
             Your case team will be in touch."
        )
    } else if reason.is_empty() {
        format!("Your case \u{201C}{case_name}\u{201D} was not accepted.")
    } else {
        format!("Your case \u{201C}{case_name}\u{201D} was not accepted. Reason: {reason}")
    };

    let callout = if accepted {
        format!(
            "Your case <strong>{case}</strong> was <strong>accepted</strong> by {actor}. \
             Your case team will be in touch.",
            case = escape(case_name),
            actor = escape(actor),
        )
    } else if reason.is_empty() {
        format!(
            "Your case <strong>{case}</strong> was <strong>not accepted</strong>.",
            case = escape(case_name),
        )
    } else {
        format!(
            "Your case <strong>{case}</strong> was <strong>not accepted</strong>.<br />Reason: {reason}",
            case = escape(case_name),
            reason = escape(reason),
        )
    };

    let cta_href = cta_url(brand, "/cases");
    let footer = notification_footer(brand);
    let heading = if accepted {
        "Your case was accepted"
    } else {
        "Your case was not accepted"
    };
    let body_note = if accepted {
        format!("Sign in to {} to see the full details.", brand.name)
    } else {
        "If you have questions about this decision, reply to this email.".to_string()
    };
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &detail_plain,
            eyebrow: "Case decision",
            heading,
            callout_html: &callout,
            cta: cta_href.as_deref().map(|u| (u, "Open the case")),
            body_note: &body_note,
            footer_html: &footer,
        },
    );
    let plain = plain(brand, &detail_plain, "/cases");

    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// Build the "you've been given access to a case" email, whose single recipient
/// is the newly assigned user (a warmer, welcome-style message).
pub fn assignment(brand: &Brand, case_name: &str, actor_name: &str) -> RenderedEmail {
    let theme = theme(NotificationKind::AccountPermissionsChanged);
    let actor = actor_or_default(actor_name);
    let subject = format!("[{}] You've been given access to {}", brand.name, case_name);

    let callout = format!(
        "<strong>{actor}</strong> gave you access to the case <strong>{case}</strong>. \
         You can now view its details, notes, evidence, and chat.",
        actor = escape(actor),
        case = escape(case_name),
    );
    let cta_href = cta_url(brand, "/cases");
    let footer = notification_footer(brand);
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &format!("{actor} gave you access to \u{201C}{case_name}\u{201D}."),
            eyebrow: "New case access",
            heading: &format!("You've been added to {}", case_name),
            callout_html: &callout,
            cta: cta_href.as_deref().map(|u| (u, "View the case")),
            body_note: &format!("Sign in to {} to see the full details.", brand.name),
            footer_html: &footer,
        },
    );
    let plain = plain(
        brand,
        &format!("{actor} gave you access to the case \u{201C}{case_name}\u{201D}."),
        "/cases",
    );

    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// Build a direct notice that an account-level role or information grant changed.
pub fn account_permissions_changed(brand: &Brand, actor_name: &str, change: &str) -> RenderedEmail {
    let theme = theme(NotificationKind::AccountPermissionsChanged);
    let actor = actor_or_default(actor_name);
    let subject = format!("[{}] Account permissions changed", brand.name);
    let detail = format!("{actor} {change}.");
    let callout = format!(
        "<strong>{actor}</strong> {change}.",
        actor = escape(actor),
        change = escape(change),
    );
    let cta_href = cta_url(brand, "/profile");
    let footer = notification_footer(brand);
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &detail,
            eyebrow: "Account permissions changed",
            heading: "Your account permissions changed",
            callout_html: &callout,
            cta: cta_href.as_deref().map(|url| (url, "Open your profile")),
            body_note: &format!("Sign in to {} to review your account.", brand.name),
            footer_html: &footer,
        },
    );
    let plain_text = plain(brand, &detail, "/profile");
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the site-admin notification that someone accepted the volunteer
/// agreement and is waiting to be approved.
pub fn volunteer_application_filed(
    brand: &Brand,
    applicant_name: &str,
    applicant_email: &str,
) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("amber-300"),
        emoji: "\u{1F64B}",
    };
    let subject = format!(
        "[{}] Approval needed: volunteer application from {}",
        brand.name, applicant_name,
    );
    let callout = format!(
        "<strong>{name}</strong> ({email}) accepted the volunteer agreement and is \
         waiting to be approved as a volunteer.",
        name = escape(applicant_name),
        email = escape(applicant_email),
    );
    let cta_href = cta_url(brand, "/admin");
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "A volunteer application is waiting for review.",
            eyebrow: "Volunteer application",
            heading: "Review requested",
            callout_html: &callout,
            cta: cta_href.as_deref().map(|url| (url, "Review application")),
            body_note: "Only a site admin can approve or deny this application.",
            footer_html: "This is a required administrative workflow notification.",
        },
    );
    let plain_text = plain(
        brand,
        &format!(
            "{applicant_name} ({applicant_email}) accepted the volunteer agreement and is \
             waiting to be approved as a volunteer."
        ),
        "/admin",
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the applicant's notification that their volunteer application was
/// approved or declined. A decline says so plainly and invites them to reapply:
/// the email is the *only* place the outcome is communicated, so it has to stand
/// on its own. `new_email`, when present, is the official address the approving
/// admin made their sign-in address.
pub fn volunteer_application_decided(
    brand: &Brand,
    approved: bool,
    decision_note: &str,
    new_email: Option<&str>,
) -> RenderedEmail {
    let theme = Theme {
        accent: if approved {
            palette::color("emerald-300")
        } else {
            palette::color("rose-400")
        },
        emoji: if approved { "\u{2705}" } else { "\u{274C}" },
    };
    let subject = format!(
        "[{}] Your volunteer application was {}",
        brand.name,
        if approved { "approved" } else { "declined" },
    );
    let note = if decision_note.is_empty() {
        String::new()
    } else {
        format!("<br><br>{}", escape(decision_note))
    };
    // Sign-in address changes are the one detail they must not miss.
    let email_change = match new_email.filter(|_| approved) {
        Some(address) => format!(
            "<br><br>Your sign-in email address is now <strong>{}</strong>. \
             Use it the next time you sign in; your password is unchanged.",
            escape(address),
        ),
        None => String::new(),
    };
    let callout = if approved {
        format!(
            "Welcome aboard \u{2014} your volunteer application has been <strong>approved</strong>. \
             Your account now has volunteer access, and a case can be assigned to you.{note}{email_change}"
        )
    } else {
        format!(
            "Thank you for offering your time. After review, your volunteer application was \
             <strong>not approved</strong> at this time.{note}"
        )
    };
    let cta_href = cta_url(brand, if approved { "/cases" } else { "/profile" });
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &format!(
                "Your volunteer application was {}.",
                if approved { "approved" } else { "declined" },
            ),
            eyebrow: "Volunteer application",
            heading: if approved {
                "Your application was approved"
            } else {
                "Your application was declined"
            },
            callout_html: &callout,
            cta: cta_href.as_deref().map(|url| {
                (
                    url,
                    if approved {
                        "Sign in"
                    } else {
                        "View your profile"
                    },
                )
            }),
            body_note: if approved {
                "Sign in to see the cases you have been given access to."
            } else {
                "You are welcome to accept the volunteer agreement and apply again."
            },
            footer_html: "This is a required notification about your account.",
        },
    );
    let plain_text = plain(
        brand,
        &if approved {
            let suffix = match new_email {
                Some(address) => format!(" Your sign-in email address is now {address}."),
                None => String::new(),
            };
            format!("Your volunteer application was approved. {decision_note}{suffix}")
        } else {
            format!("Your volunteer application was not approved at this time. {decision_note}")
        },
        if approved { "/cases" } else { "/profile" },
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the setup invitation for an approved volunteer applicant, who has no
/// account yet. Always sent to the address they *applied* with, since that is
/// the only mailbox they are known to hold: when an admin has issued them an
/// official address, this email is how they find out, and the link is how they
/// set a password and become a real account.
pub fn volunteer_applicant_approved(
    brand: &Brand,
    first_name: &str,
    setup_url: &str,
    decision_note: &str,
    new_email: Option<&str>,
) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("emerald-300"),
        emoji: "\u{2705}",
    };
    let subject = format!("[{}] Finish setting up your volunteer account", brand.name);
    let greeting = if first_name.trim().is_empty() {
        String::new()
    } else {
        format!("{}, ", escape(first_name.trim()))
    };
    let note = if decision_note.is_empty() {
        String::new()
    } else {
        format!("<br><br>{}", escape(decision_note))
    };
    // The one detail they must not miss: the address they applied with is not
    // the one that will sign them in.
    let email_change = match new_email {
        Some(address) => format!(
            "<br><br>You have been given a new email address: <strong>{}</strong>. \
             That is the address you will sign in with from now on \u{2014} not the one \
             this message was sent to.\
             <br><br>It is also your new mailbox. Open it in Outlook at \
             <a href=\"{url}\">{url}</a> with the temporary password your administrator \
             sends you separately. That password is separate from the one you choose \
             with the link below.",
            escape(address),
            url = OUTLOOK_SIGN_IN_URL,
        ),
        None => String::new(),
    };
    let callout = format!(
        "{greeting}your volunteer application has been <strong>approved</strong>. \
         One step left: follow the link below to choose a password and confirm \
         your email address, and your account is ready.{email_change}{note}"
    );
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "Your volunteer application was approved. Set your password to finish.",
            eyebrow: "Volunteer application",
            heading: "Finish setting up your account",
            callout_html: &callout,
            cta: Some((setup_url, "Complete my account")),
            body_note: "This link is personal to you and expires in 14 days.",
            footer_html: "This is a required notification about your account.",
        },
    );
    let address_line = match new_email {
        Some(address) => format!(
            "\n\nYou have been given a new email address: {address}. That is the address \
             you will sign in with from now on.\n\nIt is also your new mailbox. Open it in \
             Outlook at {OUTLOOK_SIGN_IN_URL} with the temporary password your administrator \
             sends you separately. That password is separate from the one you choose \
             with the link below."
        ),
        None => String::new(),
    };
    let plain_text = format!(
        "Your volunteer application was approved.{address_line}\n\n{decision_note}\n\n\
         Choose a password and finish setting up your account:\n{setup_url}\n\n\
         This link expires in 14 days.\n\n\u{2014} {}\n",
        brand.name,
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the decline notice for a volunteer applicant with no account. No
/// account is created, and the email is their only notice, so it says so plainly.
pub fn volunteer_applicant_declined(brand: &Brand, decision_note: &str) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("rose-400"),
        emoji: "\u{274C}",
    };
    let subject = format!("[{}] Your volunteer application", brand.name);
    let note = if decision_note.is_empty() {
        String::new()
    } else {
        format!("<br><br>{}", escape(decision_note))
    };
    let callout = format!(
        "Thank you for offering your time. After review, your volunteer application was \
         <strong>not approved</strong> at this time, and no account has been created.{note}"
    );
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "Your volunteer application was declined.",
            eyebrow: "Volunteer application",
            heading: "Your application was declined",
            callout_html: &callout,
            cta: None,
            body_note: "You are welcome to read the volunteer agreement and apply again.",
            footer_html: "This is a required notification about your application.",
        },
    );
    let plain_text = format!(
        "Your volunteer application was not approved at this time, and no account has \
         been created.\n\n{decision_note}\n\nYou are welcome to apply again.\n\n\u{2014} {}\n",
        brand.name,
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the site-admin notification for a newly filed approval request.
pub fn admin_request_filed(brand: &Brand, request: &AdminRequest) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("amber-300"),
        emoji: "\u{1F4CB}",
    };
    let subject = format!(
        "[{}] Approval needed: {} for {}",
        brand.name,
        request.kind.label(),
        request.target_user_name,
    );
    let context = request
        .case_name
        .as_deref()
        .map(|case| format!(" on <strong>{}</strong>", escape(case)))
        .unwrap_or_default();
    let note = if request.request_note.is_empty() {
        String::new()
    } else {
        format!("<br><br>Reason: {}", escape(&request.request_note))
    };
    let callout = format!(
        "<strong>{requester}</strong> requested a {kind} for <strong>{target}</strong>{context}: {change}.{note}",
        requester = escape(&request.requested_by_name),
        kind = escape(request.kind.label()),
        target = escape(&request.target_user_name),
        change = escape(&request.change_summary()),
    );
    let cta_href = cta_url(brand, "/admin");
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "An administrative request is waiting for review.",
            eyebrow: "Approval request",
            heading: "Review requested",
            callout_html: &callout,
            cta: cta_href.as_deref().map(|url| (url, "Review request")),
            body_note: "Only a site admin can approve or deny this request.",
            footer_html: "This is a required administrative workflow notification.",
        },
    );
    let plain_text = plain(
        brand,
        &format!(
            "{} requested {} for {}: {}.",
            request.requested_by_name,
            request.kind.label(),
            request.target_user_name,
            request.change_summary(),
        ),
        "/admin",
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the requester notification for a decided administrative request.
pub fn admin_request_decided(brand: &Brand, request: &AdminRequest) -> RenderedEmail {
    let approved = request.status == AdminRequestStatus::Approved;
    let status = request.status.label();
    let theme = Theme {
        accent: if approved {
            palette::color("emerald-300")
        } else {
            palette::color("rose-400")
        },
        emoji: if approved { "\u{2705}" } else { "\u{274C}" },
    };
    let subject = format!(
        "[{}] Request {}: {} for {}",
        brand.name,
        status.to_lowercase(),
        request.kind.label(),
        request.target_user_name,
    );
    let note = if request.decision_note.is_empty() {
        String::new()
    } else {
        format!("<br><br>Decision note: {}", escape(&request.decision_note))
    };
    let callout = format!(
        "The {kind} request for <strong>{target}</strong> was <strong>{status}</strong>: {change}.{note}",
        kind = escape(request.kind.label()),
        target = escape(&request.target_user_name),
        status = escape(&status.to_lowercase()),
        change = escape(&request.change_summary()),
    );
    let cta_href = cta_url(brand, "/admin");
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &format!("Your administrative request was {}.", status.to_lowercase()),
            eyebrow: "Request decision",
            heading: &format!("Request {status}"),
            callout_html: &callout,
            cta: cta_href.as_deref().map(|url| (url, "View requests")),
            body_note: "Sign in to review the request history and current access.",
            footer_html: "This is a required administrative workflow notification.",
        },
    );
    let plain_text = plain(
        brand,
        &format!(
            "The {} request for {} was {}: {}.",
            request.kind.label(),
            request.target_user_name,
            status.to_lowercase(),
            request.change_summary(),
        ),
        "/admin",
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build the administrator notification for a verified client case signup.
pub fn case_signup(
    brand: &Brand,
    client_name: &str,
    client_email: &str,
    case_name: &str,
) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("emerald-300"),
        emoji: "\u{1F4E5}",
    };
    let subject = format!("[{}] New case signup: {}", brand.name, case_name);
    let callout = format!(
        "<strong>{client}</strong> ({email}) created an account and signed up for the case <strong>{case}</strong>.",
        client = escape(client_name),
        email = escape(client_email),
        case = escape(case_name),
    );
    let cta_href = cta_url(brand, "/cases");
    let footer = notification_footer(brand);
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &format!("{client_name} signed up for \u{201C}{case_name}\u{201D}."),
            eyebrow: "New case signup",
            heading: "A client signed up",
            callout_html: &callout,
            cta: cta_href.as_deref().map(|url| (url, "View cases")),
            body_note: &format!("Sign in to {} to review the new case.", brand.name),
            footer_html: &footer,
        },
    );
    let plain_text = plain(
        brand,
        &format!(
            "{client_name} ({client_email}) created an account and signed up for the case \u{201C}{case_name}\u{201D}."
        ),
        "/cases",
    );
    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// Build one administrator-authored contact message. The body is always treated
/// as text; escaping happens before line breaks are added for HTML display.
pub fn contact_mail(brand: &Brand, subject: &str, body: &str) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("primary-500"),
        emoji: "\u{2709}\u{FE0F}",
    };
    let escaped_body = escape(body).replace("\r\n", "\n").replace('\n', "<br />");
    let footer = if brand.support_email.is_empty() {
        format!("This message was sent by {}.", escape(&brand.name))
    } else {
        format!(
            "This message was sent by {}. Questions? Contact <a href=\"mailto:{email}\" style=\"color:{muted};text-decoration:underline;\">{email}</a>.",
            escape(&brand.name),
            email = escape(&brand.support_email),
            muted = palette::muted(),
        )
    };
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: body.lines().next().unwrap_or(subject),
            eyebrow: "Message from Mommy's Heart",
            heading: subject,
            callout_html: &escaped_body,
            cta: None,
            body_note: "",
            footer_html: &footer,
        },
    );
    RenderedEmail {
        subject: subject.to_string(),
        html,
        plain_text: body.to_string(),
    }
}

/// Build the periodic admin activity digest: everything recorded since the last
/// one, grouped by category, preceded by any outbound email delivery failures
/// recorded in the same window. `extra` and `failures_extra` are how many
/// further events / failures did not fit in this batch (0 when none), so the
/// email never understates the period.
///
/// Content-free by construction: it lists who did what kind of thing to which
/// record, never note, message, or document contents.
pub fn admin_activity_digest(
    brand: &Brand,
    events: &[AdminActivityEvent],
    extra: i64,
    failures: &[EmailFailure],
    failures_extra: i64,
) -> RenderedEmail {
    let theme = theme(NotificationKind::AdminActivity);
    let count = events.len();
    let failure_count = failures.len() as i64 + failures_extra.max(0);
    let mut subject = format!(
        "[{}] Site activity: {} update{}",
        brand.name,
        count,
        if count == 1 { "" } else { "s" }
    );
    if failure_count > 0 {
        subject.push_str(&format!(
            " \u{2014} {} delivery failure{}",
            failure_count,
            if failure_count == 1 { "" } else { "s" }
        ));
    }

    // Group by category, preserving the order the categories are declared in so
    // the same headings always appear in the same sequence.
    let mut callout = String::new();
    let mut lines = Vec::new();

    // Failures come first and are highlighted: they are the part that needs
    // acting on. The heading and marker carry the signal too, so it survives
    // plain text and clients that strip color.
    if failure_count > 0 {
        let danger = palette::color("rose-400");
        callout.push_str(&format!(
            "<div style=\"margin-bottom:18px;padding:12px 14px;border-left:4px solid {danger};\
             background:{bg};\">\
             <div style=\"font-size:13px;font-weight:700;text-transform:uppercase;\
             letter-spacing:0.08em;color:{danger};\">\u{26A0} Delivery failures ({n})</div>",
            danger = danger,
            bg = palette::callout_bg(),
            n = failure_count,
        ));
        lines.push(format!(
            "!! DELIVERY FAILURES ({failure_count}) \u{2014} these emails did not send:"
        ));
        for failure in failures {
            callout.push_str(&format!(
                "<div style=\"margin-top:8px;font-size:14px;color:{text};\">\
                 <strong>{subject}</strong> \u{2014} {recipient}\
                 <div style=\"font-size:13px;color:{danger};\">{error}</div>\
                 <span style=\"font-size:12px;color:{muted};\">{context} \u{00B7} {at}</span></div>",
                text = palette::text(),
                danger = danger,
                muted = palette::muted(),
                subject = escape(&failure.subject),
                recipient = escape(&failure.recipient),
                error = escape(&failure.error),
                context = escape(&failure.context),
                at = escape(&failure.at),
            ));
            lines.push(format!(
                "  ! {} \u{2014} {} \u{2014} {} ({} \u{00B7} {})",
                failure.subject, failure.recipient, failure.error, failure.context, failure.at,
            ));
        }
        if failures_extra > 0 {
            callout.push_str(&format!(
                "<div style=\"margin-top:8px;font-size:13px;color:{muted};\">\
                 and {failures_extra} more \u{2014} see the email failure log in the app.</div>",
                muted = palette::muted(),
            ));
            lines.push(format!(
                "  and {failures_extra} more \u{2014} see the email failure log in the app."
            ));
        }
        callout.push_str("</div>");
        lines.push(String::new());
    }

    for category in AdminActivityCategory::ALL.iter().copied() {
        let grouped: Vec<&AdminActivityEvent> =
            events.iter().filter(|e| e.category == category).collect();
        if grouped.is_empty() {
            continue;
        }
        callout.push_str(&format!(
            "<div style=\"margin-top:14px;font-size:12px;font-weight:700;text-transform:uppercase;\
             letter-spacing:0.08em;color:{muted};\">{label} ({n})</div>",
            muted = palette::muted(),
            label = escape(category.label()),
            n = grouped.len(),
        ));
        lines.push(format!("{} ({})", category.label(), grouped.len()));
        for event in grouped {
            let subject_name = if event.subject_name.is_empty() {
                event.subject_id.clone()
            } else {
                event.subject_name.clone()
            };
            callout.push_str(&format!(
                "<div style=\"margin-top:6px;font-size:14px;color:{text};\">\
                 <strong>{actor}</strong> {summary} \u{2014} {name}\
                 <span style=\"color:{muted};\"> \u{00B7} {at}</span></div>",
                text = palette::text(),
                muted = palette::muted(),
                actor = escape(actor_or_default(&event.actor)),
                summary = escape(&event.summary),
                name = escape(&subject_name),
                at = escape(&event.at),
            ));
            lines.push(format!(
                "  - {} {} \u{2014} {} \u{00B7} {}",
                actor_or_default(&event.actor),
                event.summary,
                subject_name,
                event.at,
            ));
        }
    }
    if extra > 0 {
        callout.push_str(&format!(
            "<div style=\"margin-top:14px;font-size:13px;color:{muted};\">\
             and {extra} more \u{2014} see the full feed in the app.</div>",
            muted = palette::muted(),
        ));
        lines.push(format!(
            "  and {extra} more \u{2014} see the full feed in the app."
        ));
    }

    let cta_href = cta_url(brand, "/admin/activity");
    let footer = notification_footer(brand);
    // A failure count belongs in the preview line and the heading, so the alarm
    // is visible before the email is even opened.
    let failure_suffix = if failure_count > 0 {
        format!(" {failure_count} email delivery failure(s) need attention.")
    } else {
        String::new()
    };
    let heading = if failure_count > 0 {
        "Recent activity \u{2014} and delivery failures"
    } else {
        "Recent activity on the site"
    };
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: &format!(
                "{count} update(s) across cases, notes, files, and contacts.{failure_suffix}"
            ),
            eyebrow: "Admin activity alert",
            heading,
            callout_html: &callout,
            cta: cta_href
                .as_deref()
                .map(|url| (url, "Open the activity feed")),
            body_note: &format!(
                "Sign in to {} to see the full activity feed and open any record.{}",
                brand.name,
                if failure_count > 0 {
                    " Delivery failures are listed in full under Admin \u{2192} Email failures."
                } else {
                    ""
                }
            ),
            footer_html: &footer,
        },
    );

    let action = if brand.app_url.is_empty() {
        format!("Sign in to {} to see the full activity feed.", brand.name)
    } else {
        format!("Open the activity feed: {}/admin/activity", brand.app_url)
    };
    let settings = if brand.app_url.is_empty() {
        "your Settings page".to_string()
    } else {
        format!("{}/settings", brand.app_url)
    };
    let plain_text = format!(
        "Recent activity on the site \u{2014} {count} update(s).{failure_suffix}\n\n\
         {body}\n\n{action}\n\n\
         You're receiving this because of your notification settings. \
         Change what {brand} emails you about on {settings}",
        count = count,
        failure_suffix = failure_suffix,
        body = lines.join("\n"),
        action = action,
        brand = brand.name,
        settings = settings,
    );

    RenderedEmail {
        subject,
        html,
        plain_text,
    }
}

/// The full CTA url for an in-app path, or `None` when no public app URL is
/// configured (so the layout renders no button).
fn cta_url(brand: &Brand, path: &str) -> Option<String> {
    (!brand.app_url.is_empty()).then(|| format!("{}{}", brand.app_url, path))
}

/// The shared notification-email footer: the "manage your settings" sentence,
/// with a real Settings link when the app URL is known.
fn notification_footer(brand: &Brand) -> String {
    let settings = if brand.app_url.is_empty() {
        "your Settings page".to_string()
    } else {
        format!(
            "<a href=\"{url}/settings\" style=\"color:{muted};text-decoration:underline;\">your Settings page</a>",
            url = brand.app_url,
            muted = palette::muted(),
        )
    };
    format!(
        "You're receiving this because of your notification settings. Change what {brand} emails you about on {settings}.",
        brand = escape(&brand.name),
        settings = settings,
    )
}

/// Build the MFA one-time-code email. Transactional (no notification-settings
/// footer): the recipient is finishing a sign-in they just started.
pub fn auth_code(brand: &Brand, code: &str) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("primary-500"),
        emoji: "\u{1F510}",
    }; // 🔐
    let subject = format!("[{}] Your sign-in verification code", brand.name);

    let callout = format!(
        "Use this one-time code to finish signing in. It expires in 10 minutes.\
         <div style=\"margin-top:14px;font-size:32px;font-weight:700;letter-spacing:0.35em;\
         font-family:'SFMono-Regular',Consolas,'Liberation Mono',Menlo,monospace;color:{text};\">{code}</div>",
        text = palette::text(),
        code = escape(code),
    );
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "Your verification code (expires in 10 minutes).",
            eyebrow: "Security",
            heading: "Verify it's you",
            callout_html: &callout,
            cta: None,
            body_note: "",
            footer_html: "If you didn't try to sign in, you can safely ignore this email \u{2014} your account is still secure.",
        },
    );
    let plain = format!(
        "Your {brand} verification code is: {code}\n\n\
         It expires in 10 minutes.\n\n\
         If you didn't try to sign in, you can ignore this email.",
        brand = brand.name,
        code = code,
    );
    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// Build the registration email-verification code email. Transactional (no
/// notification-settings footer): the recipient is finishing a sign-up they just
/// started, and the account is not created until this code is entered.
pub fn verify_email(brand: &Brand, code: &str) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("primary-500"),
        emoji: "\u{2709}\u{FE0F}",
    }; // ✉️
    let subject = format!("[{}] Verify your email address", brand.name);

    let callout = format!(
        "Welcome! Use this one-time code to verify your email and finish creating your \
         {brand} account. It expires in 10 minutes.\
         <div style=\"margin-top:14px;font-size:32px;font-weight:700;letter-spacing:0.35em;\
         font-family:'SFMono-Regular',Consolas,'Liberation Mono',Menlo,monospace;color:{text};\">{code}</div>",
        brand = escape(&brand.name),
        text = palette::text(),
        code = escape(code),
    );
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "Your email verification code (expires in 10 minutes).",
            eyebrow: "Verify your email",
            heading: "Confirm your email address",
            callout_html: &callout,
            cta: None,
            body_note: "",
            footer_html: "If you didn't try to create an account, you can safely ignore this email \u{2014} no account will be created.",
        },
    );
    let plain = format!(
        "Welcome to {brand}! Your email verification code is: {code}\n\n\
         Enter it to finish creating your account. It expires in 10 minutes.\n\n\
         If you didn't try to create an account, you can ignore this email.",
        brand = brand.name,
        code = code,
    );
    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// Build the password-reset email. `reset_url` is the full, tokenized link the
/// recipient follows to choose a new password.
pub fn password_reset(brand: &Brand, reset_url: &str) -> RenderedEmail {
    let theme = Theme {
        accent: palette::color("primary-500"),
        emoji: "\u{1F511}",
    }; // 🔑
    let subject = format!("[{}] Reset your password", brand.name);

    let callout = format!(
        "We received a request to reset your {brand} password. \
         Choose a new one using the button below. This link expires in 1 hour.",
        brand = escape(&brand.name),
    );
    let html = layout(
        brand,
        &theme,
        &LayoutParts {
            preheader: "Reset your password (link expires in 1 hour).",
            eyebrow: "Security",
            heading: "Reset your password",
            callout_html: &callout,
            cta: Some((reset_url, "Reset password")),
            body_note: &format!(
                "If the button doesn't work, paste this link into your browser: {reset_url}"
            ),
            footer_html: "If you didn't request a password reset, you can ignore this email \u{2014} your password won't change.",
        },
    );
    let plain = format!(
        "Reset your {brand} password by opening this link (expires in 1 hour):\n{url}\n\n\
         If you didn't request this, you can ignore this email \u{2014} your password won't change.",
        brand = brand.name,
        url = reset_url,
    );
    RenderedEmail {
        subject,
        html,
        plain_text: plain,
    }
}

/// The pieces that vary between templates; everything else is shared chrome.
struct LayoutParts<'a> {
    /// Hidden preview text shown by inboxes next to the subject.
    preheader: &'a str,
    /// Small uppercase label above the heading (the category or "Security").
    eyebrow: &'a str,
    /// The prominent heading.
    heading: &'a str,
    /// Pre-escaped HTML for the highlighted callout block.
    callout_html: &'a str,
    /// Optional call-to-action button as `(full_url, label)`. `None` omits it.
    cta: Option<(&'a str, &'a str)>,
    /// A muted sentence shown under the CTA (escaped by the layout). Empty to omit.
    body_note: &'a str,
    /// Pre-rendered footer inner HTML (may contain links). Empty to omit.
    footer_html: &'a str,
}

/// Assemble the full, email-client-safe HTML document around the varying parts.
fn layout(brand: &Brand, theme: &Theme, parts: &LayoutParts) -> String {
    let (primary, muted, border) = (palette::primary(), palette::muted(), palette::border());
    let (canvas, card, text, callout_bg) = (
        palette::canvas(),
        palette::card(),
        palette::text(),
        palette::callout_bg(),
    );

    let cta = match parts.cta {
        Some((url, label)) => button(url, label, primary),
        None => String::new(),
    };

    let body_note = if parts.body_note.is_empty() {
        String::new()
    } else {
        format!(
            r#"<tr><td style="padding:8px 32px 32px 32px;font-size:14px;line-height:1.6;color:{muted};">
{note}
</td></tr>"#,
            muted = muted,
            note = escape(parts.body_note),
        )
    };

    let footer = if parts.footer_html.is_empty() {
        String::new()
    } else {
        format!(
            r#"<tr><td style="padding:20px 32px;border-top:1px solid {border};font-size:12px;line-height:1.6;color:{muted};background-color:{callout_bg};">
{footer}
</td></tr>"#,
            border = border,
            muted = muted,
            callout_bg = callout_bg,
            footer = parts.footer_html,
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<meta name="color-scheme" content="light only" />
<title>{brand_name}</title>
</head>
<body style="margin:0;padding:0;background-color:{canvas};">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;">{preheader}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background-color:{canvas};padding:24px 12px;">
<tr><td align="center">
<table role="presentation" width="600" cellpadding="0" cellspacing="0" style="width:600px;max-width:100%;background-color:{card};border:1px solid {border};border-radius:16px;overflow:hidden;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">
<tr><td style="padding:24px 32px;border-bottom:3px solid {primary};">
<span style="font-size:20px;font-weight:700;color:{text};letter-spacing:-0.01em;">
<span style="color:{primary};">&#9829;</span>&nbsp;{brand_name}</span>
</td></tr>
<tr><td style="padding:32px 32px 8px 32px;">
<div style="font-size:12px;font-weight:700;letter-spacing:0.08em;text-transform:uppercase;color:{accent};">{emoji}&nbsp;{eyebrow}</div>
<h1 style="margin:8px 0 0 0;font-size:22px;line-height:1.3;font-weight:700;color:{text};">{heading}</h1>
</td></tr>
<tr><td style="padding:16px 32px 8px 32px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background-color:{callout_bg};border-left:4px solid {accent};border-radius:8px;">
<tr><td style="padding:16px 18px;font-size:16px;line-height:1.6;color:{text};">{callout}</td></tr>
</table>
</td></tr>
{cta}
{body_note}
{footer}
</table>
</td></tr>
</table>
</body>
</html>"#,
        brand_name = escape(&brand.name),
        canvas = canvas,
        card = card,
        border = border,
        primary = primary,
        text = text,
        callout_bg = callout_bg,
        accent = theme.accent,
        emoji = theme.emoji,
        preheader = escape(parts.preheader),
        eyebrow = escape(parts.eyebrow),
        heading = escape(parts.heading),
        callout = parts.callout_html,
        cta = cta,
        body_note = body_note,
        footer = footer,
    )
}

/// A "bulletproof" table-based button that renders in Outlook as well as modern
/// clients. Returns a full table row (`<tr>`), or empty when there is no URL.
fn button(url: &str, label: &str, color: &str) -> String {
    format!(
        r#"<tr><td style="padding:20px 32px 8px 32px;">
<table role="presentation" cellpadding="0" cellspacing="0"><tr>
<td style="border-radius:10px;background-color:{color};">
<a href="{url}" style="display:inline-block;padding:12px 26px;font-size:15px;font-weight:600;color:#ffffff;text-decoration:none;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;">{label}</a>
</td></tr></table>
</td></tr>"#,
        url = escape(url),
        label = escape(label),
        color = color,
    )
}

/// The plain-text fallback body, matching the HTML content for clients that
/// prefer text. `event` is the one-sentence summary; `cta_path` deep-links.
fn plain(brand: &Brand, event: &str, cta_path: &str) -> String {
    let action = if brand.app_url.is_empty() {
        format!("Sign in to {} to see the full details.", brand.name)
    } else {
        format!(
            "Open the case: {}{}\nOr sign in to {} to see the full details.",
            brand.app_url, cta_path, brand.name
        )
    };
    let settings = if brand.app_url.is_empty() {
        "your Settings page".to_string()
    } else {
        format!("{}/settings", brand.app_url)
    };
    format!(
        "{event}\n\n{action}\n\n\
         You're receiving this because of your notification settings. \
         Change what {brand} emails you about on {settings}",
        event = event,
        action = action,
        brand = brand.name,
        settings = settings,
    )
}

/// Normalize an actor's display name, falling back to "Someone" when blank.
fn actor_or_default(actor_name: &str) -> &str {
    let trimmed = actor_name.trim();
    if trimmed.is_empty() {
        "Someone"
    } else {
        trimmed
    }
}

/// Minimal HTML escaping for interpolated, user-controlled values.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// One labelled example email, for the offline preview/test tooling.
pub struct Sample {
    /// Filename-safe identifier (used for the preview `.html` file name).
    pub key: String,
    /// Human-readable label for the gallery.
    pub label: String,
    pub email: RenderedEmail,
}

/// Render one representative email for every template with placeholder data, so
/// the whole set can be eyeballed offline without touching the email service.
/// Backs the `preview-emails` CLI command.
pub fn samples(brand: &Brand) -> Vec<Sample> {
    let case = "Rivera Family — Custody Support";
    let actor = "Jordan Alvarez";

    let mut samples: Vec<Sample> = NotificationKind::ALL
        .iter()
        .filter(|kind| {
            !matches!(
                **kind,
                NotificationKind::NewMessage
                    | NotificationKind::AccountPermissionsChanged
                    | NotificationKind::AdminRequests
                    | NotificationKind::AdminActivity
            )
        })
        .map(|kind| {
            let detail = sample_detail(*kind);
            Sample {
                key: kind.slug().to_string(),
                label: kind.label().to_string(),
                email: case_event(brand, *kind, case, actor, detail),
            }
        })
        .collect();

    samples.push(Sample {
        key: NotificationKind::NewMessage.slug().to_string(),
        label: NotificationKind::NewMessage.label().to_string(),
        email: secure_message_notice(brand),
    });
    samples.push(Sample {
        key: NotificationKind::AccountPermissionsChanged
            .slug()
            .to_string(),
        label: NotificationKind::AccountPermissionsChanged
            .label()
            .to_string(),
        email: assignment(brand, case, actor),
    });
    samples.push(Sample {
        key: "information_access_granted".to_string(),
        label: "Account permissions changed — information access".to_string(),
        email: account_permissions_changed(
            brand,
            actor,
            "granted you access to Contacts, Organizations, and Funding",
        ),
    });
    samples.push(Sample {
        key: "case_accepted".to_string(),
        label: "Case decision \u{2014} accepted".to_string(),
        email: case_decision(brand, case, actor, true, ""),
    });
    samples.push(Sample {
        key: "case_declined".to_string(),
        label: "Case decision \u{2014} declined".to_string(),
        email: case_decision(
            brand,
            case,
            actor,
            false,
            "Outside our service area; referred to Lakeside Legal Aid.",
        ),
    });
    samples.push(Sample {
        key: "case_signup".to_string(),
        label: "New case signup".to_string(),
        email: case_signup(brand, "Elena Rivera", "elena@example.org", case),
    });

    samples.push(Sample {
        key: "volunteer_applicant_approved_new_email".to_string(),
        label: "Volunteer application approved \u{2014} with a new address".to_string(),
        email: volunteer_applicant_approved(
            brand,
            "Elena",
            "https://app.example.org/volunteer-setup?token=sample",
            "",
            Some("erivera@example.org"),
        ),
    });

    samples.push(Sample {
        key: "admin_activity_digest".to_string(),
        label: "Admin activity alert \u{2014} daily digest".to_string(),
        email: admin_activity_digest(brand, &sample_activity(case, actor), 12, &[], 0),
    });
    samples.push(Sample {
        key: "admin_activity_digest_failures".to_string(),
        label: "Admin activity alert \u{2014} with delivery failures".to_string(),
        email: admin_activity_digest(
            brand,
            &sample_activity(case, actor),
            12,
            &sample_failures(),
            2,
        ),
    });

    samples.push(Sample {
        key: "contact_mail".to_string(),
        label: "Administrator contact message".to_string(),
        email: contact_mail(
            brand,
            "Community resource update",
            "Hello,\n\nWe are sharing an update about services available this month.\n\nThank you,\nMommy's Heart",
        ),
    });

    // Transactional auth emails (not tied to a NotificationKind).
    samples.push(Sample {
        key: "auth_code".to_string(),
        label: "MFA sign-in code".to_string(),
        email: auth_code(brand, "048213"),
    });
    samples.push(Sample {
        key: "verify_email".to_string(),
        label: "Registration email verification".to_string(),
        email: verify_email(brand, "048213"),
    });
    let reset_base = if brand.app_url.is_empty() {
        "https://app.example.org"
    } else {
        brand.app_url.as_str()
    };
    samples.push(Sample {
        key: "password_reset".to_string(),
        label: "Password reset".to_string(),
        email: password_reset(
            brand,
            &format!("{reset_base}/reset-password?token=example-reset-token"),
        ),
    });
    samples
}

/// A believable batch of delivery failures for the digest preview sample.
fn sample_failures() -> Vec<EmailFailure> {
    vec![
        EmailFailure {
            id: "ef-9001".to_string(),
            recipient: "intake@example.org".to_string(),
            subject: "[Mommy's Heart] New case signup".to_string(),
            context: "Case signup notification".to_string(),
            error: "550 5.1.1 recipient address rejected: user unknown".to_string(),
            at: "2025-03-04 08:41".to_string(),
        },
        EmailFailure {
            id: "ef-9002".to_string(),
            recipient: "volunteer@example.com".to_string(),
            subject: "[Mommy's Heart] Sign-in code".to_string(),
            context: "Authentication email".to_string(),
            error: "connection timed out talking to the mail service".to_string(),
            at: "2025-03-04 10:07".to_string(),
        },
    ]
}

/// A believable batch of recorded activity for the digest preview sample.
fn sample_activity(case: &str, actor: &str) -> Vec<AdminActivityEvent> {
    use crate::server_fns::admin_activity::AdminActivitySubject;

    let event =
        |category, summary: &str, subject, id: &str, name: &str, at: &str| AdminActivityEvent {
            id: format!("aa-{id}"),
            category,
            actor: actor.to_string(),
            summary: summary.to_string(),
            subject,
            subject_id: id.to_string(),
            subject_name: name.to_string(),
            at: at.to_string(),
        };
    vec![
        event(
            AdminActivityCategory::CaseCreated,
            "created the case",
            AdminActivitySubject::Case,
            "c-5001",
            case,
            "2025-03-04 09:12",
        ),
        event(
            AdminActivityCategory::CaseNote,
            "finalized a case note",
            AdminActivitySubject::Case,
            "c-5001",
            case,
            "2025-03-04 11:40",
        ),
        event(
            AdminActivityCategory::Document,
            "uploaded a file to Intake / Zoom Video",
            AdminActivitySubject::Case,
            "c-5001",
            case,
            "2025-03-04 14:02",
        ),
        event(
            AdminActivityCategory::Contact,
            "updated a contact",
            AdminActivitySubject::Contact,
            "ct-5002",
            "Dana Whitfield",
            "2025-03-04 16:25",
        ),
    ]
}

/// A believable `detail` sentence for each case-event kind, used only by the
/// preview/test samples.
fn sample_detail(kind: NotificationKind) -> &'static str {
    match kind {
        NotificationKind::NewMessage => "posted a new secure message",
        NotificationKind::CaseData => "changed the status to \u{201C}In review\u{201D}",
        NotificationKind::NoteAdded => "added a note",
        NotificationKind::EvidenceChanged => "added evidence \u{201C}hearing-notes.pdf\u{201D}",
        NotificationKind::AccountPermissionsChanged => "changed your account permissions",
        NotificationKind::AdminRequests => "filed an administrative request",
        NotificationKind::AdminActivity => "was recorded in the activity feed",
    }
}
