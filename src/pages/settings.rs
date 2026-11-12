//! The user's self-service notification settings page.
//!
//! Every signed-in account manages its own email-notification preferences here:
//! a master switch plus one toggle per [`NotificationKind`]. Loads the caller's
//! current settings after hydration and saves the whole set in one call.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::guard::require_login;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::server_fns::err_text;
use crate::server_fns::settings::{
    load_user_settings, save_user_settings, NotificationKind, UserSettings,
};
use crate::state::AppState;

/// A single labeled toggle row (checkbox + title + helper text).
#[component]
fn ToggleRow(
    title: &'static str,
    description: &'static str,
    checked: Signal<bool>,
    #[prop(into)] on_toggle: Callback<bool>,
    #[prop(into)] disabled: Signal<bool>,
) -> impl IntoView {
    view! {
        <label
            class="flex items-start gap-3 rounded-card border border-slate-800 bg-slate-900 shadow-card p-4 transition-opacity"
            class:opacity-50=move || disabled.get()
        >
            <input
                type="checkbox"
                class="mt-0.5 h-4 w-4 shrink-0 rounded border-slate-600 bg-slate-950 text-primary-500 focus:ring-2 focus:ring-primary-500/40"
                prop:checked=move || checked.get()
                prop:disabled=move || disabled.get()
                on:change=move |ev| on_toggle.run(event_target_checked(&ev))
            />
            <span class="flex flex-col">
                <span class="text-sm font-medium text-slate-100">{title}</span>
                <span class="text-xs text-slate-400">{description}</span>
            </span>
        </label>
    }
}

/// Notification settings: the signed-in user's own email preferences.
#[component]
pub fn SettingsPage() -> impl IntoView {
    let state = expect_context::<AppState>();

    // `None` until the current settings have loaded (drives the loading state).
    let settings = RwSignal::new(None::<UserSettings>);
    let load_error = RwSignal::new(None::<String>);
    let save_error = RwSignal::new(None::<String>);
    let saving = RwSignal::new(false);
    let saved = RwSignal::new(false);

    Effect::new(move |_| {
        if !state.is_authenticated() {
            return;
        }
        spawn_local(async move {
            match load_user_settings().await {
                Ok(s) => {
                    settings.set(Some(s));
                    load_error.set(None);
                }
                Err(e) => load_error.set(Some(err_text(e))),
            }
        });
    });

    require_login(state, move || {
        let master = Signal::derive(move || {
            settings
                .get()
                .map(|s| s.notifications.emails_enabled)
                .unwrap_or(true)
        });
        let set_master = move |on: bool| {
            settings.update(|opt| {
                if let Some(s) = opt {
                    s.notifications.emails_enabled = on;
                }
            });
            saved.set(false);
        };

        let rows = move || {
            // The admin-only categories would do nothing for everyone else, so
            // they are not offered as toggles they cannot act on.
            let is_admin = state.has_operations_admin_permissions();
            NotificationKind::ALL
                .into_iter()
                .filter(move |kind| is_admin || !kind.is_admin_only())
                .map(|kind| {
                    let checked = Signal::derive(move || {
                        settings
                            .get()
                            .map(|s| s.notifications.category(kind))
                            .unwrap_or(true)
                    });
                    let on_toggle = move |on: bool| {
                        settings.update(|opt| {
                            if let Some(s) = opt {
                                *s.notifications.category_mut(kind) = on;
                            }
                        });
                        saved.set(false);
                    };
                    let disabled = Signal::derive(move || !master.get());
                    view! {
                        <ToggleRow
                            title=kind.label()
                            description=kind.description()
                            checked=checked
                            on_toggle=on_toggle
                            disabled=disabled
                        />
                    }
                })
                .collect_view()
        };

        let save = move |_| {
            let Some(current) = settings.get_untracked() else {
                return;
            };
            saving.set(true);
            save_error.set(None);
            spawn_local(async move {
                match save_user_settings(current).await {
                    Ok(()) => {
                        saved.set(true);
                        save_error.set(None);
                    }
                    Err(e) => save_error.set(Some(err_text(e))),
                }
                saving.set(false);
            });
        };

        let body = move || {
            if let Some(msg) = load_error.get() {
                return view! {
                    <p class="text-sm text-rose-300">"Could not load your settings: " {msg}</p>
                }
                .into_any();
            }
            if settings.get().is_none() {
                return view! { <Loading label="Loading your settings\u{2026}" /> }.into_any();
            }
            view! {
                <div class="space-y-6">
                    <ToggleRow
                        title="Email notifications"
                        description="The master switch. Turn this off to stop all notification emails."
                        checked=master
                        on_toggle=Callback::new(set_master)
                        disabled=Signal::derive(|| false)
                    />

                    <div class="space-y-3">
                        <h2 class="text-sm font-semibold text-slate-300">"Notify me when\u{2026}"</h2>
                        {rows}
                    </div>

                    <div class="flex items-center gap-3">
                        <button
                            on:click=save
                            prop:disabled=move || saving.get()
                            class="rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-60"
                        >
                            {move || if saving.get() { "Saving\u{2026}" } else { "Save changes" }}
                        </button>
                        <Show when=move || saved.get()>
                            <span class="text-sm text-emerald-300">"Saved."</span>
                        </Show>
                        <Show when=move || save_error.get().is_some()>
                            <span class="text-sm text-rose-300">
                                {move || save_error.get().unwrap_or_default()}
                            </span>
                        </Show>
                    </div>
                </div>
            }
            .into_any()
        };

        view! {
            <Layout title="Settings".to_string()>
                <div class="max-w-2xl">
                    <p class="mb-6 text-sm text-slate-400">
                        "Choose which case activity sends you an email. These preferences apply only to your account."
                    </p>
                    {body}
                </div>
            </Layout>
        }
        .into_any()
    })
}
