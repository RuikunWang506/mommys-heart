use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;

use crate::server_fns::{auth, err_text};
use crate::state::AppState;

/// Standalone email-verification screen for a client case signup: the visitor
/// enters the one-time code emailed to confirm their address. The account and
/// the case are only created once the code is verified.
///
/// This is the only verification screen left. There used to be a second for
/// general self-service registration; that door is closed, so every pending
/// registration reaching here is a case signup.
#[component]
pub fn CaseSignupVerifyPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let navigate = use_navigate();

    let code = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let info = RwSignal::new(String::new());

    let input_class = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

    let submit = {
        let navigate = navigate.clone();
        move || {
            let navigate = navigate.clone();
            let code = code.get_untracked();
            spawn_local(async move {
                match state.verify_registration(&code).await {
                    Ok(()) => {
                        error.set(String::new());
                        navigate("/cases", Default::default());
                    }
                    Err(e) => error.set(e),
                }
            });
        }
    };

    let resend = move |_| {
        info.set(String::new());
        error.set(String::new());
        spawn_local(async move {
            match auth::resend_registration_code().await.map_err(err_text) {
                Ok(()) => info.set("We've sent a new code to your email.".into()),
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
                    <h1 class="text-lg font-semibold">"Verify your email"</h1>
                    <p class="mt-1 text-sm text-slate-400">
                        "We emailed you a 6-digit code. Enter it below to create your account and case."
                    </p>

                    <form
                        class="mt-5 space-y-4"
                        on:submit=move |ev| {
                            ev.prevent_default();
                            submit();
                        }
                    >
                        <div>
                            <label class="mb-1 block text-sm text-slate-300">"Verification code"</label>
                            <input
                                class=input_class
                                r#type="text"
                                inputmode="numeric"
                                autocomplete="one-time-code"
                                maxlength="6"
                                placeholder="123456"
                                prop:value=move || code.get()
                                on:input=move |ev| code.set(event_target_value(&ev))
                            />
                        </div>

                        <Show when=move || !info.get().is_empty()>
                            <p class="rounded-lg bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300 ring-1 ring-emerald-500/30">
                                {move || info.get()}
                            </p>
                        </Show>

                        <Show when=move || !error.get().is_empty()>
                            <p class="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-300 ring-1 ring-rose-500/30">
                                {move || error.get()}
                            </p>
                        </Show>

                        <button
                            r#type="submit"
                            class="w-full rounded-full bg-primary-500 px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-primary-600"
                        >
                            "Verify and create account and case"
                        </button>
                    </form>

                    <div class="mt-4 text-center text-sm text-slate-400">
                        "Didn't get a code? "
                        <button
                            r#type="button"
                            on:click=resend
                            class="font-medium text-primary-400 hover:text-primary-300"
                        >
                            "Resend"
                        </button>
                    </div>

                    <p class="mt-5 text-center text-sm text-slate-400">
                        <A
                            href="/case-signup"
                            attr:class="font-medium text-primary-400 hover:text-primary-300"
                        >
                            "Back to sign up"
                        </A>
                    </p>
                </div>
            </div>
        </div>
    }
}
