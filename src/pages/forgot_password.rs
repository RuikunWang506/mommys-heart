use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::server_fns::{auth, err_text};

/// Standalone "forgot password" screen. Submitting an email always shows the
/// same confirmation — the server never reveals whether the address has an
/// account — and, when it does, a tokenized reset link is emailed.
#[component]
pub fn ForgotPasswordPage() -> impl IntoView {
    let email = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let sent = RwSignal::new(false);

    let input_class = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

    let submit = move || {
        let email = email.get_untracked().trim().to_string();
        spawn_local(async move {
            match auth::request_password_reset(email).await.map_err(err_text) {
                Ok(()) => {
                    error.set(String::new());
                    sent.set(true);
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
                    <h1 class="text-lg font-semibold">"Reset your password"</h1>
                    <p class="mt-1 text-sm text-slate-400">
                        "Enter your account email and we'll send you a link to choose a new password."
                    </p>

                    <Show
                        when=move || sent.get()
                        fallback=move || {
                            view! {
                                <form
                                    class="mt-5 space-y-4"
                                    on:submit=move |ev| {
                                        ev.prevent_default();
                                        submit();
                                    }
                                >
                                    <div>
                                        <label class="mb-1 block text-sm text-slate-300">"Email"</label>
                                        <input
                                            class=input_class
                                            r#type="email"
                                            placeholder="you@example.com"
                                            prop:value=move || email.get()
                                            on:input=move |ev| email.set(event_target_value(&ev))
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
                                        "Send reset link"
                                    </button>
                                </form>
                            }
                        }
                    >
                        <p class="mt-5 rounded-lg bg-emerald-500/10 px-3 py-3 text-sm text-emerald-300 ring-1 ring-emerald-500/30">
                            "If an account exists for that email, we've sent a link to reset your password. Check your inbox."
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
