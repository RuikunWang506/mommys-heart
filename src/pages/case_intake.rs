use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_params_map;

use crate::components::case_questionnaires::CaseQuestionnaires;
use crate::components::guard::require_login;
use crate::components::layout::Layout;
use crate::components::loading::Loading;
use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::cases::{self, Case};
use crate::server_fns::err_text;
use crate::state::AppState;

#[component]
pub fn CaseIntakePage() -> impl IntoView {
    let state = expect_context::<AppState>();
    let params = use_params_map();
    let case_id = StoredValue::new(params.read_untracked().get("case_id").unwrap_or_default());
    let detail = RwSignal::new(None::<Case>);
    let loading = RwSignal::new(true);
    let error = RwSignal::new(String::new());
    let reload = RwSignal::new(0u32);

    Effect::new(move |_| {
        reload.track();
        if !state.is_authenticated() || !state.is_volunteer_or_admin() {
            loading.set(false);
            return;
        }
        let id = case_id.get_value();
        loading.set(true);
        spawn_local(async move {
            match cases::load_case(id).await {
                Ok(Some(case)) => {
                    detail.set(Some(case));
                    error.set(String::new());
                }
                Ok(None) => error.set("Case not found.".to_string()),
                Err(server_error) => error.set(err_text(server_error)),
            }
            loading.set(false);
        });
    });

    require_login(state, move || {
        view! {
            <Layout title="Case intake".to_string()>
                <div class="mx-auto max-w-4xl space-y-6">
                    <A
                        href="/cases"
                        attr:class="inline-flex items-center gap-1 text-sm font-medium text-slate-400 hover:text-slate-200"
                    >
                        "\u{2190} Back to cases"
                    </A>

                    {move || {
                        if !state.is_volunteer_or_admin() {
                            return view! {
                                <div class="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200">
                                    "Volunteer access is required to view case intake questionnaires."
                                </div>
                            }
                                .into_any();
                        }
                        if loading.get() {
                            return view! {
                                <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-4">
                                    <Loading label="Loading intake\u{2026}" />
                                </div>
                            }
                                .into_any();
                        }
                        if let Some(case) = detail.get() {
                            let can_complete = case.capabilities.contains(&CaseCapability::AddNotes)
                                && case.status.accepts_changes();
                            return view! {
                                <header class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
                                    <div class="flex flex-wrap items-center justify-between gap-3">
                                        <div>
                                            <p class="text-xs font-semibold uppercase tracking-wide text-primary-400">
                                                "Volunteer-only case intake"
                                            </p>
                                            <h1 class="mt-1 text-2xl font-semibold text-slate-100">
                                                {case.name.clone()}
                                            </h1>
                                            <p class="mt-1 text-sm text-slate-400">
                                                "Complete active questionnaires and review prior intake answers."
                                            </p>
                                        </div>
                                        <span class="rounded-full bg-amber-500/15 px-2.5 py-1 text-xs font-medium text-amber-300 ring-1 ring-amber-500/30">
                                            "Volunteers and admins only"
                                        </span>
                                    </div>
                                </header>
                                <CaseQuestionnaires
                                    case_id=case.id
                                    properties=case.properties
                                    can_complete=can_complete
                                    on_saved=Callback::new(move |_| reload.update(|value| *value += 1))
                                />
                            }
                                .into_any();
                        }
                        view! {
                            <div class="rounded-xl border border-rose-500/30 bg-rose-500/10 p-4 text-sm text-rose-200">
                                {move || {
                                    if error.get().is_empty() {
                                        "Case intake is unavailable.".to_string()
                                    } else {
                                        error.get()
                                    }
                                }}
                            </div>
                        }
                            .into_any()
                    }}
                </div>
            </Layout>
        }
        .into_any()
    })
}
