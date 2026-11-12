use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;

use crate::helpers::client_details::{ClientAgreementDetails, SERVICES_REQUESTED};
#[cfg(feature = "hydrate")]
use crate::helpers::terms::TERMS_VERSION;
use crate::helpers::terms::{
    CONSENT_CHECKBOX_LABEL, ELECTRONIC_CONSENT, ELECTRONIC_CONSENT_HEADING, ELECTRONIC_EXECUTION,
    ELECTRONIC_EXECUTION_HEADING, FOUNDATION_SIGNATORY, FOUNDATION_SIGNATORY_TITLE,
    TERMS_ATTESTATION, TERMS_MINOR_NOTICE, TERMS_SECTIONS,
};
#[cfg(feature = "hydrate")]
use crate::server_fns::{auth, err_text};

/// Where the browser remembers that the terms were accepted, so the details form
/// knows it was reached the long way round. This is a convenience for the user,
/// not a security boundary — the server independently refuses any submission
/// that does not carry the current terms version.
#[cfg(feature = "hydrate")]
const ACCEPTED_KEY: &str = "case-signup-terms-accepted";

/// Where the signed signature block is carried from step one to step two. Same
/// standing as [`ACCEPTED_KEY`]: a convenience, re-validated server-side.
#[cfg(feature = "hydrate")]
const DETAILS_KEY: &str = "case-signup-agreement-details";

/// Record acceptance of the current terms, and the block it was signed with, for
/// this browser session.
fn remember_acceptance(details: &ClientAgreementDetails) {
    #[cfg(feature = "hydrate")]
    if let Some(storage) = session_storage() {
        let _ = storage.set_item(ACCEPTED_KEY, TERMS_VERSION);
        if let Ok(json) = serde_json::to_string(details) {
            let _ = storage.set_item(DETAILS_KEY, &json);
        }
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = details;
}

/// The signature block step one recorded, if this session has one.
fn remembered_details() -> Option<ClientAgreementDetails> {
    #[cfg(feature = "hydrate")]
    {
        session_storage()
            .and_then(|storage| storage.get_item(DETAILS_KEY).ok().flatten())
            .and_then(|json| serde_json::from_str(&json).ok())
    }
    #[cfg(not(feature = "hydrate"))]
    None
}

/// Whether this browser session has accepted the terms currently in force.
fn has_accepted() -> bool {
    #[cfg(feature = "hydrate")]
    {
        session_storage()
            .and_then(|storage| storage.get_item(ACCEPTED_KEY).ok().flatten())
            .is_some_and(|version| version == TERMS_VERSION)
    }
    // Server-rendered markup is identical either way; the check runs once the
    // page hydrates, and redirects then if the terms were skipped.
    #[cfg(not(feature = "hydrate"))]
    true
}

#[cfg(feature = "hydrate")]
fn session_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|window| window.session_storage().ok().flatten())
}

/// The page header shared by both signup steps.
#[component]
fn SignupHeader(#[prop(into)] title: String) -> impl IntoView {
    view! {
        <header class="mb-7 flex items-center justify-between gap-4">
            <div class="flex items-center gap-3">
                <span class="grid h-10 w-10 place-items-center rounded-lg bg-primary-500/15 text-2xl text-primary-500">
                    "\u{2665}"
                </span>
                <div>
                    <p class="text-sm font-medium text-slate-400">"Mommy's Heart"</p>
                    <h1 class="text-2xl font-semibold text-slate-100">{title}</h1>
                </div>
            </div>
            <A href="/login" attr:class="text-sm font-medium text-primary-500 hover:text-primary-600">
                "Sign in"
            </A>
        </header>
    }
}

/// Styling shared by the signature and contact fields on step one.
const INPUT_CLASS: &str = "mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2.5 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/30 disabled:cursor-not-allowed disabled:opacity-50";
const LABEL_CLASS: &str = "block text-sm font-medium text-slate-300";
const HEADING_CLASS: &str = "mt-7 text-sm font-semibold text-slate-200";

/// The red asterisk marking a required field.
#[component]
fn Required() -> impl IntoView {
    view! { <span class="text-rose-400" aria-hidden="true">"*"</span> }
}

/// Step one of a prospective client signup: read the Terms and Conditions and
/// accept them. Nothing is sent to the server here — acceptance is carried
/// forward and recorded only if the signup itself completes, so a visitor who
/// reads the terms and leaves creates no record at all.
#[component]
pub fn CaseSignupTermsPage() -> impl IntoView {
    let navigate = use_navigate();
    let accepted = RwSignal::new(false);
    let error = RwSignal::new(String::new());
    // The acceptance control stays locked until the terms have been scrolled to
    // the bottom, so "I have read" sits under text the reader was at least shown
    // all of rather than under text they never moved past.
    let read_to_end = RwSignal::new(false);
    let terms_ref = NodeRef::<leptos::html::Div>::new();
    // The signature, contact and services block signed with the agreement.
    let details = RwSignal::new(ClientAgreementDetails::default());
    // The signature block is answerable only once the terms have been read and
    // the acknowledgment given.
    let unlocked = move || read_to_end.get() && accepted.get();

    // Toggle one entry of the services list.
    let toggle_service = move |service: &'static str, on: bool| {
        details.update(|d| {
            d.services.retain(|chosen| chosen != service);
            if on {
                d.services.push(service.to_string());
            }
        });
    };

    // Whether the terms pane is scrolled to (or within a pixel of) its end. Also
    // true when the content is short enough not to scroll at all: a pane with no
    // scrollbar can never fire a scroll event, and would otherwise lock the user
    // out of a form they have already read in full.
    let check_scrolled = move || {
        if let Some(pane) = terms_ref.get_untracked() {
            let scrolled = pane.scroll_top() as f64 + pane.client_height() as f64;
            // A pixel of slack absorbs the fractional scroll positions that zoom
            // and high-DPI displays produce, which otherwise stop just short.
            if scrolled >= pane.scroll_height() as f64 - 1.0 {
                read_to_end.set(true);
            }
        }
    };

    // Runs once after mount to catch the no-scrollbar case described above.
    Effect::new(move |_| check_scrolled());

    let submit = move || {
        if !accepted.get_untracked() {
            error.set("Please click the box to confirm you accept the terms.".to_string());
            return;
        }
        let mut signed = details.get_untracked();
        // The acknowledgment box is the consent; the date and time of submission
        // are generated server-side.
        signed.electronic_consent = accepted.get_untracked();
        let signed = signed.normalized();
        // Validated before moving on; the server runs the same checks.
        if let Err(message) = signed.validate() {
            error.set(message);
            return;
        }
        remember_acceptance(&signed);
        navigate("/case-signup/details", Default::default());
    };

    view! {
        <main class="min-h-screen bg-slate-950 px-4 py-8 text-slate-100 sm:py-12">
            <div class="mx-auto w-full max-w-4xl">
                <SignupHeader title="Terms and Conditions" />

                <form
                    class="overflow-hidden rounded-lg border border-slate-800 bg-slate-900 shadow-lg shadow-black/5"
                    on:submit=move |event| {
                        event.prevent_default();
                        submit();
                    }
                >
                    <section class="p-5 sm:p-7">
                        <div class="mb-6 flex items-start gap-3">
                            <span class="grid h-8 w-8 shrink-0 place-items-center rounded-full bg-primary-500 text-sm font-bold text-white">"1"</span>
                            <div>
                                <h2 class="text-lg font-semibold text-slate-100">"Read and accept the terms"</h2>
                                <p class="mt-1 text-sm text-slate-400">"Please read through the entire agreement before proceeding. You must accept it and sign below before you can tell us how we can help."</p>
                            </div>
                        </div>

                        <div
                            node_ref=terms_ref
                            on:scroll=move |_| check_scrolled()
                            class="max-h-[28rem] space-y-6 overflow-y-auto rounded-lg border border-slate-800 bg-slate-950 p-5 text-sm leading-relaxed text-slate-300"
                        >
                            {TERMS_SECTIONS
                                .iter()
                                .map(|section| {
                                    view! {
                                        <div class="space-y-3">
                                            <Show when=move || !section.heading.is_empty()>
                                                <h3 class="text-base font-semibold text-slate-100">{section.heading}</h3>
                                            </Show>
                                            {section
                                                .paragraphs
                                                .iter()
                                                .map(|paragraph| view! { <p>{*paragraph}</p> })
                                                .collect_view()}
                                        </div>
                                    }
                                })
                                .collect_view()}
                            <p class="pt-2 text-xs font-medium text-slate-500">"— End of Terms and Conditions —"</p>
                        </div>

                        <Show when=move || !read_to_end.get()>
                            <p class="mt-3 text-xs text-red-400/90">
                                "Scroll to the end of the terms to continue."
                            </p>
                        </Show>
                    </section>

                    <section class="border-t border-slate-800 p-5 sm:p-7">
                        <p class="text-sm text-slate-400">{TERMS_ATTESTATION}</p>

                        <h3 class="mt-6 text-sm font-semibold uppercase tracking-wide text-slate-200">
                            {ELECTRONIC_CONSENT_HEADING}
                        </h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">{ELECTRONIC_CONSENT}</p>

                        <label
                            class="mt-5 flex items-start gap-3 text-sm"
                            class=("text-slate-200", move || read_to_end.get())
                            class=("text-slate-500", move || !read_to_end.get())
                        >
                            <input
                                class="mt-0.5 h-4 w-4 rounded border-slate-700 bg-slate-950 text-primary-500 focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50"
                                type="checkbox"
                                disabled=move || !read_to_end.get()
                                prop:checked=move || accepted.get()
                                on:change=move |event| {
                                    accepted.set(event_target_checked(&event));
                                    error.set(String::new());
                                }
                            />
                            <span>{CONSENT_CHECKBOX_LABEL}</span>
                        </label>
                        <p class="mt-3 text-xs text-slate-500">{TERMS_MINOR_NOTICE}</p>

                        <Show when=move || read_to_end.get() && !accepted.get()>
                            <p class="mt-3 text-xs text-slate-400">
                                "Click the box above to continue."
                            </p>
                        </Show>
                    </section>

                    <section class="border-t border-slate-800 p-5 sm:p-7">
                        <h2 class="text-lg font-semibold text-slate-100">
                            "Recipient\u{2019}s information and signature"
                        </h2>
                        <p class="mt-1 text-sm text-slate-400">
                            "Fields marked with " <Required /> " are required. The date and time of submission are recorded automatically."
                        </p>

                        <Show when=move || !unlocked()>
                            <p class="mb-5 mt-4 rounded-lg bg-slate-800/60 px-3 py-2 text-xs text-slate-400">
                                "Read the agreement to the end and accept it above to fill this in."
                            </p>
                        </Show>

                        <div class="mt-4 grid gap-5 sm:grid-cols-2">
                            <div>
                                <label class=LABEL_CLASS>
                                    "Recipient\u{2019}s full legal name " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().legal_name
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.legal_name = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Electronic signature (type full legal name) " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().signature_name
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.signature_name = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Email address " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    type="email"
                                    autocomplete="email"
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().email
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.email = value);
                                    }
                                />
                                <p class="mt-1 text-xs text-slate-500">
                                    "This becomes your sign-in username."
                                </p>
                            </div>
                            <div>
                                <label class=LABEL_CLASS>"Date and time of submission"</label>
                                <input
                                    class=INPUT_CLASS
                                    disabled=true
                                    prop:value="Recorded automatically when you submit"
                                />
                            </div>
                        </div>

                        <h3 class=HEADING_CLASS>"If signing for a minor"</h3>
                        <p class="mt-1 text-xs text-slate-500">
                            "Optional \u{2014} leave blank if you are signing for yourself."
                        </p>
                        <div class="mt-4 grid gap-5 sm:grid-cols-2">
                            <div>
                                <label class=LABEL_CLASS>"Minor recipient\u{2019}s full legal name"</label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().minor_name
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.minor_name = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Parent/legal guardian\u{2019}s full legal name"
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().guardian_name
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.guardian_name = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>"Relationship to minor"</label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().guardian_relationship
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.guardian_relationship = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Electronic signature (type parent/legal guardian\u{2019}s full legal name)"
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().guardian_signature
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.guardian_signature = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Parent/legal guardian\u{2019}s email address"
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    type="email"
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().guardian_email
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.guardian_email = value);
                                    }
                                />
                            </div>
                        </div>

                        <h3 class=HEADING_CLASS>"Mommy\u{2019}s Heart acceptance"</h3>
                        <div class="mt-4 grid gap-5 sm:grid-cols-2">
                            <div>
                                <label class=LABEL_CLASS>
                                    "Authorized representative\u{2019}s full legal name"
                                </label>
                                <input class=INPUT_CLASS disabled=true prop:value=FOUNDATION_SIGNATORY />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>"Title"</label>
                                <input
                                    class=INPUT_CLASS
                                    disabled=true
                                    prop:value=FOUNDATION_SIGNATORY_TITLE
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Electronic signature (type full legal name)"
                                </label>
                                <input class=INPUT_CLASS disabled=true prop:value=FOUNDATION_SIGNATORY />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>"Date"</label>
                                <input
                                    class=INPUT_CLASS
                                    disabled=true
                                    prop:value="Countersigned when your agreement is filed"
                                />
                            </div>
                        </div>

                        <h3 class=HEADING_CLASS>{ELECTRONIC_EXECUTION_HEADING}</h3>
                        <p class="mt-2 text-sm leading-relaxed text-slate-400">{ELECTRONIC_EXECUTION}</p>
                    </section>

                    <section class="border-t border-slate-800 p-5 sm:p-7">
                        <h2 class="text-lg font-semibold text-slate-100">
                            "Recipient\u{2019}s contact information"
                        </h2>

                        <Show when=move || !unlocked()>
                            <p class="mb-5 mt-4 rounded-lg bg-slate-800/60 px-3 py-2 text-xs text-slate-400">
                                "Read the agreement to the end and accept it above to fill this in."
                            </p>
                        </Show>

                        <div class="mt-4 grid gap-5 sm:grid-cols-2">
                            <div>
                                <label class=LABEL_CLASS>
                                    "Date of birth " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    type="date"
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().date_of_birth
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.date_of_birth = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Phone " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    placeholder="(000) 000-0000"
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().phone
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.phone = value);
                                    }
                                />
                            </div>
                        </div>

                        <h3 class=HEADING_CLASS>"EMERGENCY CONTACT INFORMATION"</h3>
                        <div class="mt-4 grid gap-5 sm:grid-cols-2">
                            <div>
                                <label class=LABEL_CLASS>
                                    "Name " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().emergency_name
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.emergency_name = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Relationship to recipient " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().emergency_relationship
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.emergency_relationship = value);
                                    }
                                />
                            </div>
                            <div>
                                <label class=LABEL_CLASS>
                                    "Phone " <Required />
                                </label>
                                <input
                                    class=INPUT_CLASS
                                    placeholder="(000) 000-0000"
                                    prop:disabled=move || !unlocked()
                                    prop:value=move || details.get().emergency_phone
                                    on:input=move |event| {
                                        let value = event_target_value(&event);
                                        details.update(|d| d.emergency_phone = value);
                                    }
                                />
                            </div>
                        </div>

                        <h3 class=HEADING_CLASS>
                            "SERVICES REQUESTED (check all that apply) " <Required />
                        </h3>
                        <div class="mt-3 grid gap-2 sm:grid-cols-2">
                            {SERVICES_REQUESTED
                                .iter()
                                .map(|service| {
                                    let service = *service;
                                    view! {
                                        <label class="flex items-start gap-3 text-sm text-slate-300">
                                            <input
                                                class="mt-0.5 h-4 w-4 rounded border-slate-700 bg-slate-950 text-primary-500 focus:ring-2 focus:ring-primary-500/40 disabled:cursor-not-allowed disabled:opacity-50"
                                                type="checkbox"
                                                prop:disabled=move || !unlocked()
                                                prop:checked=move || {
                                                    details.get().services.iter().any(|c| c == service)
                                                }
                                                on:change=move |event| {
                                                    toggle_service(service, event_target_checked(&event));
                                                }
                                            />
                                            <span>{service}</span>
                                        </label>
                                    }
                                })
                                .collect_view()}
                        </div>

                        <Show when=move || !error.get().is_empty()>
                            <p class="mt-5 rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-300 ring-1 ring-rose-500/30">
                                {move || error.get()}
                            </p>
                        </Show>

                        <div class="mt-7 flex flex-wrap items-center justify-end gap-3">
                            <A
                                href="/login"
                                attr:class="min-h-10 rounded-full border border-slate-700 px-5 py-2.5 text-sm font-semibold text-slate-300 hover:bg-slate-800"
                            >
                                "Decline"
                            </A>
                            <button
                                type="submit"
                                disabled=move || !accepted.get() || !read_to_end.get()
                                class="min-w-36 rounded-full bg-primary-500 px-6 py-3 text-sm font-bold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                                "ACCEPT AND CONTINUE"
                            </button>
                        </div>
                    </section>
                </form>
            </div>
        </main>
    }
}

struct AccountFields {
    first_name: String,
    last_name: String,
    email: String,
    password: String,
    password_confirmation: String,
}

async fn start_case_signup(
    account: AccountFields,
    summary: String,
    agreement: ClientAgreementDetails,
) -> Result<(), String> {
    #[cfg(feature = "hydrate")]
    {
        return auth::register_case_signup(
            account.first_name,
            account.last_name,
            account.email,
            account.password,
            account.password_confirmation,
            summary,
            TERMS_VERSION.to_string(),
            agreement,
        )
        .await
        .map_err(err_text);
    }

    #[cfg(not(feature = "hydrate"))]
    {
        let _ = (
            account.first_name,
            account.last_name,
            account.email,
            account.password,
            account.password_confirmation,
            summary,
        );
        let _ = agreement;
        Ok(())
    }
}

/// Step two of a prospective client signup: the case details and the account to
/// sign in with. Reached only after the terms have been accepted; arriving here
/// directly bounces back to them.
#[component]
pub fn CaseSignupDetailsPage() -> impl IntoView {
    let navigate = use_navigate();
    let first_name = RwSignal::new(String::new());
    let last_name = RwSignal::new(String::new());
    let email = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let password_confirmation = RwSignal::new(String::new());
    let summary = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let pending = RwSignal::new(false);

    // Send anybody who skipped the terms back to read them.
    Effect::new({
        let navigate = navigate.clone();
        move |_| {
            if !has_accepted() {
                navigate("/case-signup", Default::default());
            }
        }
    });

    // The block signed on step one. The email is prefilled from it so the
    // signature and the account it creates cannot disagree.
    let agreement = RwSignal::new(ClientAgreementDetails::default());
    Effect::new(move |_| {
        if let Some(signed) = remembered_details() {
            email.set(signed.email.clone());
            agreement.set(signed);
        }
    });

    let input_class = "mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2.5 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/30";
    let label_class = "block text-sm font-medium text-slate-300";

    let submit = {
        let navigate = navigate.clone();
        move || {
            if pending.get_untracked() {
                return;
            }
            if password.get_untracked() != password_confirmation.get_untracked() {
                error.set("The passwords do not match.".to_string());
                return;
            }
            let account = AccountFields {
                first_name: first_name.get_untracked(),
                last_name: last_name.get_untracked(),
                email: email.get_untracked(),
                password: password.get_untracked(),
                password_confirmation: password_confirmation.get_untracked(),
            };
            let summary = summary.get_untracked().trim().to_string();
            if summary.is_empty() {
                error.set("Please tell us briefly what you need help with.".to_string());
                return;
            }
            let navigate = navigate.clone();
            pending.set(true);
            error.set(String::new());
            spawn_local(async move {
                match start_case_signup(account, summary, agreement.get_untracked()).await {
                    Ok(()) => navigate("/case-signup/verify", Default::default()),
                    Err(message) => {
                        error.set(message);
                        pending.set(false);
                    }
                }
            });
        }
    };

    view! {
        <main class="min-h-screen bg-slate-950 px-4 py-8 text-slate-100 sm:py-12">
            <div class="mx-auto w-full max-w-4xl">
                <SignupHeader title="Prospective client case signup" />

                <p class="mb-5 rounded-lg bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300 ring-1 ring-emerald-500/30">
                    "Service Agreement signed. Now tell us how we can help."
                </p>

                <form
                    class="overflow-hidden rounded-lg border border-slate-800 bg-slate-900 shadow-lg shadow-black/5"
                    on:submit=move |event| {
                        event.prevent_default();
                        submit();
                    }
                >
                    <section class="p-5 sm:p-7">
                        <div class="mb-6 flex items-start gap-3">
                            <span class="grid h-8 w-8 shrink-0 place-items-center rounded-full bg-primary-500 text-sm font-bold text-white">"2"</span>
                            <div>
                                <h2 class="text-lg font-semibold text-slate-100">"Tell Us How We Can Help"</h2>
                                <p class="mt-1 text-sm text-slate-400">
                                    "A brief description is all we need for now. A member of our team will guide you through the full intake process."
                                </p>
                            </div>
                        </div>

                        <div>
                            <label class=label_class>
                                "What do you need help with? "
                                <span class="text-rose-400" aria-hidden="true">"*"</span>
                            </label>
                            <textarea
                                class=input_class
                                rows="4"
                                required
                                placeholder="For example: I need help with custody of my two children."
                                prop:value=move || summary.get()
                                on:input=move |event| summary.set(event_target_value(&event))
                            ></textarea>
                            <p class="mt-2 text-xs text-slate-500">
                                "Please do not include any information you would prefer to discuss privately. We will ask for additional details when we speak with you."
                            </p>
                        </div>
                    </section>

                    <section class="border-t border-slate-800 p-5 sm:p-7">
                        <div class="mb-6 flex items-start gap-3">
                            <span class="grid h-8 w-8 shrink-0 place-items-center rounded-full bg-primary-500 text-sm font-bold text-white">"3"</span>
                            <div>
                                <h2 class="text-lg font-semibold text-slate-100">"Provide account info"</h2>
                                <p class="mt-1 text-sm text-slate-400">"Your email address will be your sign-in username."</p>
                            </div>
                        </div>
                        <div class="grid gap-5 sm:grid-cols-2">
                            <div>
                                <label class=label_class>
                                    "First name "
                                    <span class="text-rose-400" aria-hidden="true">"*"</span>
                                </label>
                                <input class=input_class autocomplete="given-name" required prop:value=move || first_name.get() on:input=move |event| first_name.set(event_target_value(&event)) />
                            </div>
                            <div>
                                <label class=label_class>
                                    "Last name "
                                    <span class="text-rose-400" aria-hidden="true">"*"</span>
                                </label>
                                <input class=input_class autocomplete="family-name" required prop:value=move || last_name.get() on:input=move |event| last_name.set(event_target_value(&event)) />
                            </div>
                            <div class="sm:col-span-2">
                                <label class=label_class>
                                    "Email (sign-in username) "
                                    <span class="text-rose-400" aria-hidden="true">"*"</span>
                                </label>
                                <input class=input_class type="email" autocomplete="email" required prop:value=move || email.get() on:input=move |event| email.set(event_target_value(&event)) />
                            </div>
                            <div>
                                <label class=label_class>
                                    "Password "
                                    <span class="text-rose-400" aria-hidden="true">"*"</span>
                                </label>
                                <input class=input_class type="password" autocomplete="new-password" required minlength="8" prop:value=move || password.get() on:input=move |event| password.set(event_target_value(&event)) />
                                <p class="mt-1 text-xs text-slate-500">"At least 8 characters."</p>
                            </div>
                            <div>
                                <label class=label_class>
                                    "Re-enter password "
                                    <span class="text-rose-400" aria-hidden="true">"*"</span>
                                </label>
                                <input class=input_class type="password" autocomplete="new-password" required minlength="8" prop:value=move || password_confirmation.get() on:input=move |event| password_confirmation.set(event_target_value(&event)) />
                            </div>
                        </div>

                        <Show when=move || !error.get().is_empty()>
                            <p class="mt-5 rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-300 ring-1 ring-rose-500/30">
                                {move || error.get()}
                            </p>
                        </Show>

                        <div class="mt-7 flex justify-end">
                            <button
                                type="submit"
                                disabled=move || pending.get()
                                class="min-w-36 rounded-full bg-primary-500 px-6 py-3 text-sm font-bold text-white hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-60"
                            >
                                {move || if pending.get() { "SUBMITTING..." } else { "SUBMIT" }}
                            </button>
                        </div>
                    </section>
                </form>
            </div>
        </main>
    }
}
