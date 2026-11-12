use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Title};
use leptos_router::components::{Redirect, Route, Router, Routes, A};
use leptos_router::path;

use crate::pages::{
    admin::{
        AdminActivityPage, AdminCaseDetailPage, AdminCasesPage, AdminDashboardPage,
        AdminUserDetailPage, AdminUsersPage,
    },
    bulk_properties::BulkPropertiesPage,
    case_intake::CaseIntakePage,
    case_notes::{CaseNoteDetailPage, NewCaseNotePage},
    case_signup::{CaseSignupDetailsPage, CaseSignupTermsPage},
    cases::{CaseHomePage, NewCasePage},
    contact_categories::ContactCategoriesPage,
    contact_mail::ContactMailPage,
    contacts::{ContactsPage, NewContactPage},
    crm_import::CrmImportPage,
    forgot_password::ForgotPasswordPage,
    funding::FundingPage,
    inbox::InboxPage,
    login::LoginPage,
    mfa::MfaVerifyPage,
    organizations::OrganizationsPage,
    profile::ProfilePage,
    reports::{NewReportPage, ReportsPage, SavedReportPage},
    reset_password::ResetPasswordPage,
    settings::SettingsPage,
    verify_email::CaseSignupVerifyPage,
    volunteer_agreement::{VolunteerAgreementPage, VolunteerSignupPage},
    volunteer_setup::VolunteerSetupPage,
};
use crate::state::AppState;

/// The HTML document shell rendered on the server around the hydrated app.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <link rel="icon" type="image/svg+xml" href="/favicon.svg" />
                // Derived from the build's output name, so the link is correct
                // whatever the app was built as.
                <link rel="stylesheet" id="leptos" href=options.css_path() />
                <AutoReload options=options.clone() />
                <HydrationScripts options=options.clone() />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

/// Sends the visitor to the right landing page based on their session. Reactive
/// on the auth phase, which resolves asynchronously in the browser.
#[component]
fn HomeRedirect() -> impl IntoView {
    let state = expect_context::<AppState>();
    move || {
        if state.is_authenticated() {
            view! { <Redirect path="/cases" /> }.into_any()
        } else if !state.auth_resolved.get() {
            // Render nothing while page loads
            ().into_any()
        } else {
            view! { <Redirect path="/login" /> }.into_any()
        }
    }
}

/// Friendly full-page 404 shown when no route matches.
#[component]
fn NotFoundPage() -> impl IntoView {
    view! {
        <div class="min-h-screen bg-slate-950 text-slate-100 flex items-center justify-center px-4">
            <div class="w-full max-w-md text-center">
                <div class="mb-6 flex items-center justify-center gap-2">
                    <span class="grid h-10 w-10 place-items-center rounded-xl bg-primary-500/20 text-2xl text-primary-400">
                        "\u{2665}"
                    </span>
                    <span class="text-xl font-semibold tracking-tight">"Mommy's Heart"</span>
                </div>

                <div class="rounded-2xl border border-slate-800 bg-slate-900 p-8 shadow-xl shadow-black/30">
                    <p class="text-6xl font-bold tracking-tight text-primary-400">"404"</p>
                    <h1 class="mt-4 text-lg font-semibold text-slate-100">"Page not found"</h1>
                    <p class="mt-2 text-sm text-slate-400">
                        "The page you're looking for doesn't exist or may have moved."
                    </p>
                    <A
                        href="/"
                        attr:class="mt-6 inline-block rounded-full border border-primary-500/40 bg-primary-500/10 px-4 py-2 text-sm font-medium text-primary-300 hover:bg-primary-500/20"
                    >
                        "Back to home"
                    </A>
                </div>
            </div>
        </div>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    let state = AppState::new();
    provide_context(state);
    state.restore_session();

    // Refresh unread notifications every 5 minutes
    Effect::new(move |_| {
        set_interval(
            move || state.refresh_unread(),
            std::time::Duration::from_secs(300),
        );
    });

    view! {
        <Title text="Mommy's Heart" />

        <Router>
            <Routes fallback=|| view! { <NotFoundPage /> }>
                <Route path=path!("/") view=HomeRedirect />
                <Route path=path!("/login") view=LoginPage />
                <Route path=path!("/mfa") view=MfaVerifyPage />
                <Route path=path!("/forgot-password") view=ForgotPasswordPage />
                <Route path=path!("/reset-password") view=ResetPasswordPage />
                // The only two ways to create an account: a client opening a
                // case, and a volunteer applying. There is deliberately no
                // general registration route.
                <Route path=path!("/case-signup") view=CaseSignupTermsPage />
                <Route path=path!("/case-signup/details") view=CaseSignupDetailsPage />
                <Route path=path!("/case-signup/verify") view=CaseSignupVerifyPage />
                <Route path=path!("/volunteer-signup") view=VolunteerSignupPage />
                <Route path=path!("/volunteer-setup") view=VolunteerSetupPage />
                <Route path=path!("/cases") view=CaseHomePage />
                <Route path=path!("/cases/new") view=NewCasePage />
                <Route path=path!("/cases/:case_id/intake") view=CaseIntakePage />
                <Route path=path!("/cases/:case_id/notes/new") view=NewCaseNotePage />
                <Route path=path!("/cases/:case_id/notes/:note_id") view=CaseNoteDetailPage />
                <Route path=path!("/contacts") view=ContactsPage />
                <Route path=path!("/contacts/mail") view=ContactMailPage />
                // Declared before `/contacts/:id`, which would otherwise match
                // "categories" as a contact id.
                <Route path=path!("/contacts/categories") view=ContactCategoriesPage />
                <Route path=path!("/contacts/new") view=NewContactPage />
                <Route path=path!("/contacts/:id") view=ContactsPage />
                <Route path=path!("/organizations") view=OrganizationsPage />
                <Route path=path!("/organizations/:id") view=OrganizationsPage />
                <Route path=path!("/properties/bulk") view=BulkPropertiesPage />
                <Route path=path!("/import") view=CrmImportPage />
                <Route path=path!("/funding") view=FundingPage />
                <Route path=path!("/funding/:id") view=FundingPage />
                <Route path=path!("/inbox") view=InboxPage />
                <Route path=path!("/reports") view=ReportsPage />
                <Route path=path!("/reports/new") view=NewReportPage />
                <Route path=path!("/reports/:id") view=SavedReportPage />
                <Route path=path!("/settings") view=SettingsPage />
                <Route path=path!("/profile") view=ProfilePage />
                <Route path=path!("/volunteer-agreement") view=VolunteerAgreementPage />
                <Route path=path!("/profile/:id") view=ProfilePage />
                <Route path=path!("/admin") view=AdminDashboardPage />
                <Route path=path!("/admin/cases") view=AdminCasesPage />
                <Route path=path!("/admin/cases/:id") view=AdminCaseDetailPage />
                <Route path=path!("/admin/users") view=AdminUsersPage />
                <Route path=path!("/admin/users/:id") view=AdminUserDetailPage />
                <Route path=path!("/admin/activity") view=AdminActivityPage />
            </Routes>
        </Router>
    }
}
