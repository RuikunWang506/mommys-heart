use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_query_map;

use crate::server_fns::{auth, err_text};

/// Standalone "set a new password" screen, reached from the tokenized link in
/// the reset email (`/reset-password?token=…`). Validates the token server-side
/// and, on success, invalidates the account's other sessions.
#[component]
pub fn ResetPasswordPage() -> impl IntoView {
    let query = use_query_map();
    let token = move || query.read().get("token").unwrap_or_default();

    let password = RwSignal::new(String::new());
    let confirm = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let done = RwSignal::new(false);

    let input_class = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

    let submit = move || {
        let token = token();
        let password = password.get_untracked();
        let confirm = confirm.get_untracked();
        if token.is_empty() {
            error.set("This reset link is missing its token. Please request a new one.".into());
            return;
        }
        if password.is_empty() {
            error.set("Please choose a new password.".into());
            return;
        }
        if password != confirm {
            error.set("The passwords don't match.".into());
            return;
        }
        spawn_local(async move {
            match auth::reset_password(token, password)
                .await
                .map_err(err_text)
            {
                Ok(()) => {
                    error.set(String::new());
                    done.set(true);
                }
                Err(e) => error.set(e),
            }
        });
    };

    view! {
        <div class="min-h-screen bg-slate-950 text-slate-100 flex items-center justify-center px-4">
            <div class="w-full max-w-md">
                <div class="mb-6 flex items-center justify-center gap-2">
                    <span class="grid h-10 w-10 place-items-center rounded-xl bg-primary-500/20 text-2xl text-primary-400">
                        "\u{2665}"
                    </span>
                    <span class="text-xl font-semibold tracking-tight">"Mommy's Heart"</span>
                </div>

                <div class="rounded-2xl border border-slate-800 bg-slate-900 p-6 shadow-xl shadow-black/30">
                    <h1 class="text-lg font-semibold">"Choose a new password"</h1>

                    <Show
                        when=move || done.get()
                        fallback=move || {
                            view! {
                                <p class="mt-1 text-sm text-slate-400">
                                    "Enter a new password for your account."
                                </p>
                                <form
                                    class="mt-5 space-y-4"
                                    on:submit=move |ev| {
                                        ev.prevent_default();
                                        submit();
                                    }
                                >
                                    <div>
                                        <label class="mb-1 block text-sm text-slate-300">"New password"</label>
                                        <input
                                            class=input_class
                                            r#type="password"
                                            autocomplete="new-password"
                                            placeholder="Choose a password"
                                            prop:value=move || password.get()
                                            on:input=move |ev| password.set(event_target_value(&ev))
                                        />
                                    </div>
                                    <div>
                                        <label class="mb-1 block text-sm text-slate-300">"Confirm new password"</label>
                                        <input
                                            class=input_class
                                            r#type="password"
                                            autocomplete="new-password"
                                            placeholder="Re-enter your password"
                                            prop:value=move || confirm.get()
                                            on:input=move |ev| confirm.set(event_target_value(&ev))
                                        />
                                    </div>

                                    <Show when=move || !error.get().is_empty()>
                                        <p class="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-300 ring-1 ring-rose-500/30">
                                            {move || error.get()}
                                        </p>
                                    </Show>

                                    <button
                                        r#type="submit"
                                        class="w-full rounded-full bg-primary-500 px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-primary-600"
                                    >
                                        "Update password"
                                    </button>
                                </form>
                            }
                        }
                    >
                        <p class="mt-5 rounded-lg bg-emerald-500/10 px-3 py-3 text-sm text-emerald-300 ring-1 ring-emerald-500/30">
                            "Your password has been updated. You can now sign in with your new password."
                        </p>
                    </Show>

                    <p class="mt-5 text-center text-sm text-slate-400">
                        <A
                            href="/login"
                            attr:class="font-medium text-primary-400 hover:text-primary-300"
                        >
                            "Back to sign in"
                        </A>
                    </p>
                </div>
            </div>
        </div>
    }
}
