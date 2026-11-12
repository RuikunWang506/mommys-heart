//! Route-backed administrative workspaces: cases, users, and admin tools.

use leptos::prelude::*;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_params_map;

use crate::components::admin_activity::AdminActivityFeed;
use crate::components::admin_manage_cases::ManageCases;
use crate::components::admin_manage_users::ManageUsers;
use crate::components::email_failures::EmailFailureLog;
use crate::components::guard::require_operations_admin;
use crate::components::layout::Layout;
use crate::state::AppState;

/// Which workspace a route shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AdminWorkspace {
    Cases,
    Users,
    Activity,
}

/// `/admin` has one obvious starting place.
#[component]
pub fn AdminDashboardPage() -> impl IntoView {
    view! { <Redirect path="/admin/cases" /> }
}

#[component]
pub fn AdminCasesPage() -> impl IntoView {
    admin_page(AdminWorkspace::Cases, None)
}

#[component]
pub fn AdminCaseDetailPage() -> impl IntoView {
    let params = use_params_map();
    move || {
        let case_id = params.read().get("id").filter(|id| !id.trim().is_empty());
        admin_page(AdminWorkspace::Cases, case_id)
    }
}

#[component]
pub fn AdminUsersPage() -> impl IntoView {
    admin_page(AdminWorkspace::Users, None)
}

/// The recorded site-activity feed — the in-app half of the "Admin activity
/// alert" notification category.
#[component]
pub fn AdminActivityPage() -> impl IntoView {
    admin_page(AdminWorkspace::Activity, None)
}

#[component]
pub fn AdminUserDetailPage() -> impl IntoView {
    let params = use_params_map();
    move || {
        let user_id = params.read().get("id").filter(|id| !id.trim().is_empty());
        admin_page(AdminWorkspace::Users, user_id)
    }
}

fn admin_page(workspace: AdminWorkspace, selected_id: Option<String>) -> AnyView {
    let state = expect_context::<AppState>();
    let reload = RwSignal::new(0u32);
    let selected_id = StoredValue::new(selected_id);

    require_operations_admin(state, move || {
        let actor = state
            .current_user_summary
            .get()
            .expect("admin guard requires a current user");
        let is_site_admin = actor.role.is_site_admin();
        let actor_user_id = actor.id;

        let title = match workspace {
            AdminWorkspace::Cases => "Manage Cases",
            AdminWorkspace::Users => "Manage Users",
            AdminWorkspace::Activity => "Site Activity",
        };

        let content = match workspace {
            AdminWorkspace::Cases => view! {
                <ManageCases
                    is_site_admin=is_site_admin
                    reload=reload
                    selected_case_id=selected_id.get_value()
                />
            }
            .into_any(),
            AdminWorkspace::Users => view! {
                <ManageUsers
                    is_site_admin=is_site_admin
                    actor_user_id=actor_user_id
                    reload=reload
                    selected_user_id=selected_id.get_value()
                />
            }
            .into_any(),
            AdminWorkspace::Activity => view! {
                <section class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
                    <h2 class="text-sm font-semibold text-slate-200">"Site activity"</h2>
                    <p class="mt-1 text-xs text-slate-500">
                        "New cases, case notes, information edits, documents, and contact changes \
                         across the site, newest first \u{2014} drawn from the same change history \
                         each record keeps. Administrators who have the \
                         \u{201C}Admin activity alert\u{201D} notification turned on also get this \
                         as a daily summary email."
                    </p>
                    <AdminActivityFeed />
                </section>
            }
            .into_any(),
        };

        let tools = matches!(workspace, AdminWorkspace::Activity)
            .then(|| view! { <AdminTools /> });

        view! {
            <Layout title=title.to_string()>
                {content}
                {tools}
            </Layout>
        }
        .into_any()
    })
}

#[component]
fn AdminTools() -> impl IntoView {
    let open = RwSignal::new(false);
    view! {
        <section class="mt-10 border-t border-slate-800 pt-6">
            <h2>
                <button
                    type="button"
                    on:click=move |_| open.update(|value| *value = !*value)
                    aria-expanded=move || open.get().to_string()
                    aria-controls="admin-tools-content"
                    class="flex w-full items-center justify-between gap-3 text-left"
                >
                    <span>
                        <span class="block text-base font-semibold text-slate-100">"Admin tools"</span>
                        <span class="mt-1 block text-xs font-normal text-slate-500">
                            "Operational diagnostics kept separate from case and user management."
                        </span>
                    </span>
                    <span class="text-xs font-medium text-slate-400">
                        {move || if open.get() { "Hide" } else { "Show" }}
                    </span>
                </button>
            </h2>
            <Show when=move || open.get()>
                <div id="admin-tools-content" class="mt-4 rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
                    <h3 class="text-sm font-semibold text-slate-200">"Email delivery failures"</h3>
                    <p class="mt-1 text-xs text-slate-500">
                        "Outbound emails that failed to send, newest first."
                    </p>
                    <EmailFailureLog />
                </div>
            </Show>
        </section>
    }
}
