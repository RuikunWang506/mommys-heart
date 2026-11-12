//! `/volunteer-setup?token=…`: the last step of the public volunteer path.
//!
//! An approved applicant arrives here from the link emailed to the address they
//! applied with. The token is their only credential — there is no account yet —
//! so the page states plainly which address will sign them in (which may not be
//! the one that received the link) and takes a password. Submitting creates the
//! account and signs them in.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::{use_navigate, use_query_map};

use crate::server_fns::err_text;
use crate::server_fns::volunteer_applicants::{
    complete_volunteer_setup, volunteer_setup_details, VolunteerSetup,
};
use crate::state::AppState;

#[component]
pub fn VolunteerSetupPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    // Stored rather than captured directly, so the closures below stay `Copy`
    // and can be called from the reactive view more than once.
    let navigate = StoredValue::new(use_navigate());
    let query = use_query_map();

    let token = Memo::new(move |_| query.get().get("token").unwrap_or_default());
    let setup = RwSignal::new(None::<VolunteerSetup>);
    // `None` while the token is still being checked, so the form is not flashed
    // up before we know the link is good.
    let load_error = RwSignal::new(None::<String>);
    let password = RwSignal::new(String::new());
    let confirmation = RwSignal::new(String::new());
    let error = RwSignal::new(String::new());
    let submitting = RwSignal::new(false);

    Effect::new(move |_| {
        let token = token.get();
        if token.is_empty() {
            load_error.set(Some(
                "This setup link is missing its token. Please use the link from your email."
                    .to_string(),
            ));
            return;
        }
        spawn_local(async move {
            match volunteer_setup_details(token).await {
                Ok(details) => {
                    setup.set(Some(details));
                    load_error.set(None);
                }
                Err(e) => load_error.set(Some(err_text(e))),
            }
        });
    });

    let submit = move || {
            if submitting.get_untracked() {
                return;
            }
            let (password, confirmation) =
                (password.get_untracked(), confirmation.get_untracked());
            if password != confirmation {
                error.set("The passwords do not match.".to_string());
                return;
            }
            submitting.set(true);
            error.set(String::new());
            let navigate = navigate.get_value();
            let token = token.get_untracked();
            spawn_local(async move {
                match complete_volunteer_setup(token, password, confirmation).await {
                    Ok(user) => {
                        // Signed in by the same call, so the app chrome is
                        // correct the moment we land.
                        state.current_user_summary.set(Some(user.into()));
                        state.refresh_badges();
                        navigate("/cases", Default::default());
                    }
                    Err(e) => {
                        error.set(err_text(e));
                        submitting.set(false);
                    }
                }
            });
    };

    let input_class = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";

    let card = move || {
        if let Some(message) = load_error.get() {
            return view! {
                <div class="rounded-2xl border border-slate-800 bg-slate-900 p-6 shadow-xl shadow-black/30">
                    <h1 class="text-lg font-semibold">"This link doesn't work"</h1>
                    <p class="mt-2 text-sm text-rose-300">{message}</p>
                    <p class="mt-4 text-sm text-slate-400">
                        "If your link has expired, ask an administrator to send you a new one."
                    </p>
                    <A
                        href="/login"
                        attr:class="mt-5 inline-block text-sm font-medium text-primary-400 hover:text-primary-300"
                    >
                        "Back to sign in"
                    </A>
                </div>
            }
            .into_any();
        }
        let Some(details) = setup.get() else {
            return view! {
                <p class="text-center text-sm text-slate-400">"Checking your link\u{2026}"</p>
            }
            .into_any();
        };

        let greeting = if details.first_name.is_empty() {
            "Welcome \u{2014} one step left".to_string()
        } else {
            format!("Welcome, {} \u{2014} one step left", details.first_name)
        };
        let sign_in_email = details.sign_in_email.clone();

        view! {
            <div class="rounded-2xl border border-slate-800 bg-slate-900 p-6 shadow-xl shadow-black/30">
                <h1 class="text-lg font-semibold">{greeting}</h1>
                <p class="mt-1 text-sm text-slate-400">
                    "Your volunteer application was approved. Choose a password and your account is ready."
                </p>

                <div class="mt-5 rounded-lg border border-slate-800 bg-slate-950 px-3 py-2.5">
                    <p class="text-xs uppercase tracking-wide text-slate-500">
                        "You will sign in with"
                    </p>
                    <p class="mt-1 break-all text-sm font-semibold text-slate-100">
                        {sign_in_email}
                    </p>
                    <Show when=move || details.email_changed>
                        <p class="mt-2 text-xs text-amber-300">
                            "This is a new address we've given you. It is not the address this link \
                             was emailed to \u{2014} use the one above from now on."
                        </p>
                    </Show>
                </div>

                <form
                    class="mt-5 space-y-4"
                    on:submit=move |event| {
                        event.prevent_default();
                        submit();
                    }
                >
                    <div>
                        <label class="mb-1 block text-sm text-slate-300">"Choose a password"</label>
                        <input
                            class=input_class
                            type="password"
                            autocomplete="new-password"
                            required
                            minlength="8"
                            prop:value=move || password.get()
                            on:input=move |event| password.set(event_target_value(&event))
                        />
                        <p class="mt-1 text-xs text-slate-500">"At least 8 characters."</p>
                    </div>
                    <div>
                        <label class="mb-1 block text-sm text-slate-300">"Re-enter password"</label>
                        <input
                            class=input_class
                            type="password"
                            autocomplete="new-password"
                            required
                            minlength="8"
                            prop:value=move || confirmation.get()
                            on:input=move |event| confirmation.set(event_target_value(&event))
                        />
                    </div>

                    <Show when=move || !error.get().is_empty()>
                        <p class="rounded-lg bg-rose-500/10 px-3 py-2 text-sm text-rose-300 ring-1 ring-rose-500/30">
                            {move || error.get()}
                        </p>
                    </Show>

                    <button
                        type="submit"
                        disabled=move || submitting.get()
                        class="w-full rounded-full bg-primary-500 px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-primary-600 disabled:cursor-not-allowed disabled:opacity-60"
                    >
                        {move || if submitting.get() {
                            "Creating your account\u{2026}"
                        } else {
                            "Create my account"
                        }}
                    </button>
                </form>
            </div>
        }
        .into_any()
    };

    view! {
        <div class="flex min-h-screen items-center justify-center bg-slate-950 px-4 text-slate-100">
            <div class="w-full max-w-md">
                <div class="mb-6 flex items-center justify-center gap-2">
                    <span class="grid h-10 w-10 place-items-center rounded-xl bg-primary-500/20 text-2xl text-primary-400">
                        "\u{2665}"
                    </span>
                    <span class="text-xl font-semibold tracking-tight">"Mommy's Heart"</span>
                </div>
                {card}
            </div>
        </div>
    }
}
