use leptos::prelude::*;
use leptos_router::components::{Redirect, A};
use leptos_router::hooks::{use_location, use_navigate};

use crate::state::AppState;

#[derive(Clone, Copy)]
enum NavBadge {
    UnreadMessages,
    PendingCaseWork,
    PendingUserWork,
}

/// A top-navbar link that highlights when its route is active.
#[component]
fn NavLink(
    #[prop(into)] href: String,
    label: &'static str,
    #[prop(optional)] badge: Option<NavBadge>,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let location = use_location();
    let match_on = href.clone();
    let active = move || {
        let path = location.pathname.get();
        if match_on == "/" {
            path == "/"
        } else {
            path.starts_with(&match_on)
        }
    };
    let unread = move || match badge {
        Some(NavBadge::UnreadMessages) => state.total_unread(),
        Some(NavBadge::PendingCaseWork) => {
            state.cases_pending_review.get() + state.admin_case_request_pending.get()
        }
        Some(NavBadge::PendingUserWork) => {
            state.volunteer_requests_pending.get()
                + state.admin_role_request_pending.get()
                + state.admin_information_request_pending.get()
        }
        None => 0,
    };

    view! {
        <A
            href=href
            attr:class=move || {
                let base = "px-2.5 py-2 rounded-lg text-sm font-medium whitespace-nowrap transition-colors";
                if active() {
                    format!("{base} bg-primary-500/15 text-primary-300")
                } else if unread() > 0 {
                    format!("{base} text-primary-300 hover:bg-slate-800")
                } else {
                    format!("{base} text-slate-300 hover:bg-slate-800 hover:text-slate-100")
                }
            }
        >
            {move || {
                let n = unread();
                if n > 0 {
                    view! {
                        <span class="inline-flex items-center gap-1.5">
                            {label}
                                <span class="inline-flex min-w-[1.25rem] items-center justify-center rounded-full bg-primary-500 px-1.5 py-0.5 text-[0.65rem] font-semibold leading-none text-white">
                                    {n}
                                </span>
                        </span>
                    }
                    .into_any()
                } else {
                    label.into_any()
                }
            }}
        </A>
    }
}

/// App shell: a dark top navbar plus the page content. Redirects to `/login`
/// when there is no signed-in user.
#[component]
pub fn Layout(#[prop(into)] title: String, children: Children) -> impl IntoView {
    let state = expect_context::<AppState>();
    let navigate = use_navigate();

    let user = state.current_user_summary.get();
    if user.is_none() {
        return view! { <Redirect path="/login" /> }.into_any();
    }
    let user = user.unwrap();
    let role = user.role;
    let user_name = user.full_name();
    let my_profile_href = format!("/profile/{}", user.id);
    let profile_title = format!("{user_name}, {}", role.label());
    let initials = crate::helpers::format::initials(&user_name);
    let profile_label = format!("My Profile: {profile_title}");

    let logout = move |_| {
        state.logout();
        navigate("/login", Default::default());
    };

    let menu_open = RwSignal::new(false);
    let toggle_menu = move |_| menu_open.update(|open| *open = !*open);
    let close_menu = move |_| menu_open.set(false);
    let has_operations_admin_permissions = role.has_operations_admin_permissions();
    // Reports read across the whole database, so the link matches the
    // operations-admin gate on the page and on the server function behind it.
    let can_build_reports = has_operations_admin_permissions;

    view! {
        <div class="min-h-screen bg-slate-950 text-slate-100">
            <header class="sticky top-0 z-20 border-b border-slate-800 bg-slate-900/80 backdrop-blur">
                <div class="mx-auto max-w-7xl px-4 sm:px-6">
                    <div class="flex h-16 items-center gap-2 xl:gap-3">
                        <A href="/" attr:class="flex items-center gap-2 shrink-0">
                            <span class="grid h-8 w-8 place-items-center rounded-lg bg-primary-500/20 text-lg text-primary-400">
                                "\u{2665}"
                            </span>
                            <span class="hidden font-semibold tracking-tight min-[440px]:inline">
                                "Mommy's Heart"
                            </span>
                        </A>

                        <nav class="hidden xl:flex items-center gap-0.5">
                            <NavLink href="/cases" label="Cases" />
                            <NavLink href="/inbox" label="Case Chat" badge=NavBadge::UnreadMessages />
                            {move || if state.has_information_management_access() {
                                view! {
                                    <NavLink href="/contacts" label="Contacts" />
                                    <NavLink href="/organizations" label="Organizations" />
                                    <NavLink href="/funding" label="Funding" />
                                }.into_any()
                            } else {
                                ().into_any()
                            }}
                            {if has_operations_admin_permissions {
                                view! {
                                    <NavLink
                                        href="/admin/cases"
                                        label="Manage Cases"
                                        badge=NavBadge::PendingCaseWork
                                    />
                                    <NavLink
                                        href="/admin/users"
                                        label="Manage Users"
                                        badge=NavBadge::PendingUserWork
                                    />
                                    <NavLink href="/admin/activity" label="Site Activity" />
                                }
                                .into_any()
                            } else {
                                ().into_any()
                            }}
                            {if can_build_reports {
                                view! { <NavLink href="/reports" label="Reports" /> }.into_any()
                            } else {
                                ().into_any()
                            }}
                            <NavLink href="/settings" label="Settings" />
                        </nav>

                        <div class="ml-auto flex items-center gap-2 sm:gap-3">
                            <A
                                href=my_profile_href.clone()
                                attr:title=profile_title
                                attr:class="hidden sm:inline-flex h-[38px] w-[38px] shrink-0 items-center justify-center rounded-full bg-[#e3effa] text-sm font-bold text-[#1a5d99] hover:ring-2 hover:ring-[#cfe3f5] transition"
                            >
                                <span aria-hidden="true">{initials}</span>
                                <span class="sr-only">{profile_label}</span>
                            </A>
                            <button
                                on:click=logout
                                class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium whitespace-nowrap text-slate-300 hover:bg-slate-800 hover:text-slate-100 transition-colors"
                            >
                                "Log out"
                            </button>
                            <button
                                on:click=toggle_menu
                                aria-label="Toggle navigation menu"
                                class="xl:hidden grid h-9 w-9 place-items-center rounded-full border border-slate-700 text-slate-300 hover:bg-slate-800 hover:text-slate-100 transition-colors"
                            >
                                {move || if menu_open.get() { "\u{2715}" } else { "\u{2630}" }}
                            </button>
                        </div>
                    </div>

                    <nav
                        class="xl:hidden overflow-hidden flex flex-col gap-1 transition-all"
                        class:hidden=move || !menu_open.get()
                        style="padding-bottom: 0.75rem"
                        on:click=close_menu
                    >
                        <NavLink href="/cases" label="Cases" />
                        <NavLink href="/inbox" label="Case Chat" badge=NavBadge::UnreadMessages />
                        {move || if state.has_information_management_access() {
                            view! {
                                <NavLink href="/contacts" label="Contacts" />
                                <NavLink href="/organizations" label="Organizations" />
                                <NavLink href="/funding" label="Funding" />
                            }.into_any()
                        } else {
                            ().into_any()
                        }}
                        {if has_operations_admin_permissions {
                            view! {
                                <NavLink
                                    href="/admin/cases"
                                    label="Manage Cases"
                                    badge=NavBadge::PendingCaseWork
                                />
                                <NavLink
                                    href="/admin/users"
                                    label="Manage Users"
                                    badge=NavBadge::PendingUserWork
                                />
                                <NavLink href="/admin/activity" label="Site Activity" />
                            }
                            .into_any()
                        } else {
                            ().into_any()
                        }}
                        {if can_build_reports {
                            view! { <NavLink href="/reports" label="Reports" /> }.into_any()
                        } else {
                            ().into_any()
                        }}
                        <NavLink href="/settings" label="Settings" />
                        <NavLink href=my_profile_href label="My Profile" />
                    </nav>
                </div>
            </header>

            <main class="mx-auto max-w-7xl px-4 sm:px-6 py-8">
                <h1 class="text-2xl font-semibold tracking-tight mb-6">{title}</h1>
                {children()}
            </main>

            <footer class="mx-auto max-w-7xl px-4 sm:px-6 py-6 text-right text-xs text-slate-500">
                {format!("v{}", env!("CARGO_PKG_VERSION"))}
            </footer>
        </div>
    }
    .into_any()
}
