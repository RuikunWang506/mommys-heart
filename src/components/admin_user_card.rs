//! The management card for a single user: their global account role and their
//! per-case access.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::change_log::ChangeLog;
use crate::server_fns::audit::AuditScope;
use crate::server_fns::capabilities::{CaseCapability, CasePreset};
use crate::server_fns::cases::CaseSummary;
use crate::server_fns::err_text;
use crate::server_fns::users::{AccountRole, User};
use crate::state::AppState;

/// A single case assignment being edited in the case-access draft.
#[derive(Clone)]
struct DraftAssignment {
    case_id: String,
    name: String,
    caps: RwSignal<Vec<CaseCapability>>,
    removed: RwSignal<bool>,
}

/// Order-insensitive equality of two capability sets.
fn same_caps(a: &[CaseCapability], b: &[CaseCapability]) -> bool {
    a.len() == b.len() && a.iter().all(|capability| b.contains(capability))
}

fn badge(classes: &str) -> String {
    format!("inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium {classes}")
}

#[component]
pub fn UserCard(
    user: User,
    reload: RwSignal<u32>,
    actor_user_id: String,
    is_site_admin: bool,
) -> impl IntoView {
    let state = expect_context::<AppState>();
    let user_id = user.id.clone();
    let full_name = user.full_name();
    let email = user.email.clone();
    let current_role = user.role;
    let can_edit_capabilities_directly = is_site_admin || user_id == actor_user_id;
    let can_request_role_change = !is_site_admin && user_id != actor_user_id;
    let is_deactivated = user.role.is_deactivated();
    let deactivation = user.deactivation.clone();

    // --- account role ------------------------------------------------------
    let role_target = StoredValue::new(user_id.clone());
    let selected_role = RwSignal::new(current_role.slug().to_string());
    let role_note = RwSignal::new(String::new());
    let role_busy = RwSignal::new(false);
    let role_feedback = RwSignal::new(None::<Result<String, String>>);
    let role_select_id = StoredValue::new(format!("account-role-select-{user_id}"));
    let role_note_id = StoredValue::new(format!("account-role-note-{user_id}"));

    // --- information management access ------------------------------------
    let information_target = StoredValue::new(user_id.clone());
    let information_actor = StoredValue::new(actor_user_id.clone());
    let current_information_access = user.information_management_access;
    let information_enabled = RwSignal::new(current_information_access);
    let information_busy = RwSignal::new(false);
    let information_feedback = RwSignal::new(None::<Result<String, String>>);
    let information_request_note = RwSignal::new(String::new());
    let information_note_id = StoredValue::new(format!("information-note-{user_id}"));

    let apply_information_access = move |_| {
        if information_busy.get_untracked()
            || information_enabled.get_untracked() == current_information_access
        {
            return;
        }
        information_busy.set(true);
        information_feedback.set(None);
        spawn_local(async move {
            let result = crate::server_fns::users::set_information_management_access(
                information_target.get_value(),
                information_enabled.get_untracked(),
            )
            .await
            .map(|_| "Information access updated.".to_string())
            .map_err(err_text);
            match result {
                Ok(message) => {
                    information_feedback.set(Some(Ok(message)));
                    reload.update(|value| *value += 1);
                    if information_target.get_value() == information_actor.get_value() {
                        state.current_user_summary.update(|current| {
                            if let Some(current) = current {
                                current.information_management_access =
                                    information_enabled.get_untracked();
                            }
                        });
                    }
                }
                Err(message) => information_feedback.set(Some(Err(message))),
            }
            information_busy.set(false);
        });
    };

    let request_information_access = move |_| {
        if information_busy.get_untracked()
            || current_information_access
            || !current_role.has_volunteer_privileges()
        {
            return;
        }
        information_busy.set(true);
        information_feedback.set(None);
        spawn_local(async move {
            let result = crate::server_fns::admin_requests::request_user_information_access(
                information_target.get_value(),
                information_request_note.get_untracked(),
            )
            .await
            .map(|_| "Information access request submitted.".to_string())
            .map_err(err_text);
            match result {
                Ok(message) => {
                    information_feedback.set(Some(Ok(message)));
                    information_request_note.set(String::new());
                    state.refresh_badges();
                }
                Err(message) => information_feedback.set(Some(Err(message))),
            }
            information_busy.set(false);
        });
    };

    // --- email address -----------------------------------------------------
    let email_target = StoredValue::new(user_id.clone());
    let current_email = StoredValue::new(user.email.clone());
    let email_editing = RwSignal::new(false);
    let email_value = RwSignal::new(String::new());
    let email_confirm = RwSignal::new(String::new());
    let email_busy = RwSignal::new(false);
    let email_feedback = RwSignal::new(None::<Result<String, String>>);
    let email_input_id = StoredValue::new(format!("account-email-{user_id}"));
    let email_confirm_id = StoredValue::new(format!("account-email-confirm-{user_id}"));

    let apply_email_change = move |_| {
        if email_busy.get_untracked() {
            return;
        }
        let wanted = email_value.get_untracked();
        let confirm = email_confirm.get_untracked();
        if wanted.trim().is_empty() {
            email_feedback.set(Some(Err("Please enter an email address.".to_string())));
            return;
        }
        // The server checks this too; catching it here saves a round-trip.
        if wanted.trim().to_lowercase() != confirm.trim().to_lowercase() {
            email_feedback.set(Some(Err(
                "The two email addresses do not match.".to_string()
            )));
            return;
        }
        email_busy.set(true);
        email_feedback.set(None);
        spawn_local(async move {
            let outcome =
                crate::server_fns::users::set_user_email(email_target.get_value(), wanted, confirm)
                    .await
                    .map(|_| "Email address updated.".to_string())
                    .map_err(err_text);
            match outcome {
                Ok(message) => {
                    email_feedback.set(Some(Ok(message)));
                    email_editing.set(false);
                    email_value.set(String::new());
                    email_confirm.set(String::new());
                    reload.update(|value| *value += 1);
                }
                Err(message) => email_feedback.set(Some(Err(message))),
            }
            email_busy.set(false);
        });
    };

    // --- account status (deactivation) ------------------------------------
    let status_target = StoredValue::new(user_id.clone());
    let status_busy = RwSignal::new(false);
    let status_feedback = RwSignal::new(None::<Result<String, String>>);
    // Two-step, like the case-withdraw control: reveal, type an optional
    // reason, then confirm. Retiring an account should not be one stray click.
    let status_confirming = RwSignal::new(false);
    let status_reason = RwSignal::new(String::new());
    let status_reason_id = StoredValue::new(format!("account-status-reason-{user_id}"));
    let is_self_account = user_id == actor_user_id;

    let apply_status_change = move |deactivate: bool| {
        move |_| {
            if status_busy.get_untracked() {
                return;
            }
            status_busy.set(true);
            status_feedback.set(None);
            spawn_local(async move {
                let result = crate::server_fns::users::set_account_deactivated(
                    status_target.get_value(),
                    deactivate,
                    status_reason.get_untracked(),
                )
                .await
                .map(|_| {
                    if deactivate {
                        "Account deactivated.".to_string()
                    } else {
                        "Account reactivated.".to_string()
                    }
                })
                .map_err(err_text);
                match result {
                    Ok(message) => {
                        status_feedback.set(Some(Ok(message)));
                        status_confirming.set(false);
                        status_reason.set(String::new());
                        reload.update(|value| *value += 1);
                    }
                    Err(message) => status_feedback.set(Some(Err(message))),
                }
                status_busy.set(false);
            });
        }
    };
    let deactivate_account = apply_status_change(true);
    let reactivate_account = apply_status_change(false);

    let apply_role_change = move |_| {
        if role_busy.get_untracked() {
            return;
        }
        let Some(role) = AccountRole::from_slug(&selected_role.get_untracked()) else {
            role_feedback.set(Some(Err("Choose a valid account role.".to_string())));
            return;
        };
        if role == current_role {
            return;
        }

        role_busy.set(true);
        role_feedback.set(None);
        let target_user_id = role_target.get_value();
        let note = role_note.get_untracked();
        spawn_local(async move {
            let outcome = if is_site_admin {
                crate::server_fns::users::set_user_role(target_user_id, role)
                    .await
                    .map(|_| "Account role updated.".to_string())
                    .map_err(err_text)
            } else {
                crate::server_fns::admin_requests::request_user_role_change(
                    target_user_id,
                    role,
                    note,
                )
                .await
                .map(|_| "Role request submitted.".to_string())
                .map_err(err_text)
            };
            match outcome {
                Ok(message) => {
                    role_feedback.set(Some(Ok(message)));
                    role_note.set(String::new());
                    selected_role.set(current_role.slug().to_string());
                    if is_site_admin {
                        reload.update(|value| *value += 1);
                    }
                    if role_target.get_value() == information_actor.get_value() {
                        state.current_user_summary.update(|current| {
                            if let Some(current) = current {
                                current.role = role;
                            }
                        });
                    }
                    state.refresh_badges();
                }
                Err(message) => role_feedback.set(Some(Err(message))),
            }
            role_busy.set(false);
        });
    };

    // --- case access -------------------------------------------------------
    let new_case = RwSignal::new(String::new());
    let new_case_label = RwSignal::new(String::new());
    let new_preset = RwSignal::new(CasePreset::Viewer.slug().to_string());
    let case_query = RwSignal::new(String::new());
    let case_results = RwSignal::new(Vec::<CaseSummary>::new());
    let picker_open = RwSignal::new(false);
    let active_case_result = RwSignal::new(None::<usize>);
    let case_search_generation = RwSignal::new(0u64);
    let case_search_id = StoredValue::new(format!("case-search-{user_id}"));
    let case_results_id = StoredValue::new(format!("case-results-{user_id}"));
    let case_preset_id = StoredValue::new(format!("case-preset-{user_id}"));

    Effect::new(move |_| {
        if !picker_open.get() {
            return;
        }
        let query = case_query.get();
        case_search_generation.update(|generation| *generation += 1);
        let generation = case_search_generation.get_untracked();
        spawn_local(async move {
            let response = crate::server_fns::cases::admin_search_cases(query).await;
            if case_search_generation.get_untracked() != generation {
                return;
            }
            match response {
                Ok(cases) => case_results.set(cases),
                Err(_) => case_results.set(Vec::new()),
            }
            active_case_result.set(None);
        });
    });

    let user_sv = StoredValue::new(user_id.clone());
    let owner = StoredValue::new(Owner::current().expect("component owner"));
    let make_draft = move |case_id: String, name: String, caps: Vec<CaseCapability>| {
        owner.with_value(|current_owner| {
            current_owner.with(|| DraftAssignment {
                case_id,
                name,
                caps: RwSignal::new(caps),
                removed: RwSignal::new(false),
            })
        })
    };

    let case_names = RwSignal::new(std::collections::HashMap::<String, String>::new());
    let assigned_ids: Vec<String> = user
        .assigned_cases
        .iter()
        .map(|assignment| assignment.case_id.clone())
        .collect();
    {
        let assigned_ids = assigned_ids.clone();
        Effect::new(move |_| {
            let ids = assigned_ids.clone();
            if ids.is_empty() {
                return;
            }
            spawn_local(async move {
                if let Ok(cases) = crate::server_fns::cases::admin_cases_by_ids(ids).await {
                    case_names.update(|names| {
                        for case in cases {
                            names.insert(case.id, case.name);
                        }
                    });
                }
            });
        });
    }

    let case_name = move |case_id: &str| -> String {
        case_names
            .with(|names| names.get(case_id).cloned())
            .unwrap_or_else(|| case_id.to_string())
    };

    let originals = StoredValue::new(user.assigned_cases.clone());
    let editing = RwSignal::new(false);
    let draft: RwSignal<Vec<DraftAssignment>> = RwSignal::new(Vec::new());
    let case_feedback = RwSignal::new(None::<Result<String, String>>);
    let saving = RwSignal::new(false);
    let capability_request_note = RwSignal::new(String::new());
    let capability_note_id = StoredValue::new(format!("capability-note-{user_id}"));

    let begin_edit = move |_| {
        let rows = originals
            .get_value()
            .into_iter()
            .map(|assignment| {
                make_draft(
                    assignment.case_id.clone(),
                    case_name(&assignment.case_id),
                    assignment.capabilities,
                )
            })
            .collect::<Vec<_>>();
        draft.set(rows);
        case_feedback.set(None);
        capability_request_note.set(String::new());
        picker_open.set(false);
        new_case.set(String::new());
        new_case_label.set(String::new());
        case_query.set(String::new());
        editing.set(true);
    };

    let cancel_edit = move |_| {
        editing.set(false);
        case_feedback.set(None);
        capability_request_note.set(String::new());
        picker_open.set(false);
        new_case.set(String::new());
        new_case_label.set(String::new());
        case_query.set(String::new());
    };

    let add_to_draft = move |_| {
        let case_id = new_case.get_untracked();
        if case_id.is_empty() {
            return;
        }
        if let Some(row) = draft
            .get_untracked()
            .into_iter()
            .find(|row| row.case_id == case_id)
        {
            row.removed.set(false);
        } else {
            let preset =
                CasePreset::from_slug(&new_preset.get_untracked()).unwrap_or(CasePreset::Viewer);
            let label = new_case_label.get_untracked();
            let name = if label.is_empty() {
                case_name(&case_id)
            } else {
                label
            };
            draft
                .update(|rows| rows.push(make_draft(case_id.clone(), name, preset.capabilities())));
        }
        new_case.set(String::new());
        new_case_label.set(String::new());
        case_query.set(String::new());
        picker_open.set(false);
    };

    let save_edit = move |_| {
        let user_id = user_sv.get_value();
        let originals = originals.get_value();
        let rows = draft.get_untracked();

        let mut changes: Vec<(String, Option<Vec<CaseCapability>>)> = Vec::new();
        for row in &rows {
            let caps = row.caps.get_untracked();
            let removed = row.removed.get_untracked();
            match originals
                .iter()
                .find(|assignment| assignment.case_id == row.case_id)
            {
                Some(assignment) => {
                    if removed || caps.is_empty() {
                        changes.push((row.case_id.clone(), None));
                    } else if !same_caps(&assignment.capabilities, &caps) {
                        changes.push((row.case_id.clone(), Some(caps)));
                    }
                }
                None => {
                    if !removed && !caps.is_empty() {
                        changes.push((row.case_id.clone(), Some(caps)));
                    }
                }
            }
        }

        if changes.is_empty() {
            editing.set(false);
            case_feedback.set(None);
            return;
        }

        saving.set(true);
        case_feedback.set(None);
        spawn_local(async move {
            let outcome = if can_edit_capabilities_directly {
                crate::server_fns::users::save_case_capabilities(user_id, changes)
                    .await
                    .map(|_| "Case access updated.".to_string())
                    .map_err(err_text)
            } else {
                let note = capability_request_note.get_untracked();
                crate::server_fns::admin_requests::request_user_case_capabilities(
                    user_id, changes, note,
                )
                .await
                .map(|_| "Case access request submitted.".to_string())
                .map_err(err_text)
            };
            match outcome {
                Ok(message) => {
                    case_feedback.set(Some(Ok(message)));
                    capability_request_note.set(String::new());
                    editing.set(false);
                    if can_edit_capabilities_directly {
                        reload.update(|value| *value += 1);
                    }
                    state.refresh_badges();
                }
                Err(message) => case_feedback.set(Some(Err(message))),
            }
            saving.set(false);
        });
    };

    let case_result_list = move || {
        if !picker_open.get() {
            return ().into_any();
        }
        let items = case_results.get();
        if items.is_empty() {
            return view! {
                <div
                    id=case_results_id.get_value()
                    role="listbox"
                    class="absolute z-10 mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-xs text-slate-500"
                >
                    "No matching cases."
                </div>
            }
            .into_any();
        }
        let rows = items
            .into_iter()
            .enumerate()
            .map(|(index, case)| {
                let case_id = case.id.clone();
                let label = format!("{} ({})", case.name, case.id);
                let option_id = format!("{}-{index}", case_results_id.get_value());
                let select = {
                    let case_id = case_id.clone();
                    let name = case.name.clone();
                    move |_| {
                        new_case.set(case_id.clone());
                        new_case_label.set(name.clone());
                        picker_open.set(false);
                        active_case_result.set(None);
                    }
                };
                view! {
                    <button
                        id=option_id
                        type="button"
                        role="option"
                        tabindex="-1"
                        aria-selected=move || (active_case_result.get() == Some(index)).to_string()
                        on:click=select
                        class=move || if active_case_result.get() == Some(index) {
                            "block w-full truncate bg-slate-800 px-3 py-1.5 text-left text-sm text-slate-100"
                        } else {
                            "block w-full truncate px-3 py-1.5 text-left text-sm text-slate-200 hover:bg-slate-800"
                        }
                    >
                        {label}
                    </button>
                }
                .into_any()
            })
            .collect_view();
        view! {
            <div
                id=case_results_id.get_value()
                role="listbox"
                class="absolute z-10 mt-1 max-h-48 w-full overflow-y-auto rounded-xl border border-slate-700 bg-slate-900"
            >
                {rows}
            </div>
        }
        .into_any()
    };

    let email_section = move || {
        if !is_site_admin {
            return view! {
                <section class="mt-4 border-t border-slate-800 pt-4">
                    <h3 class="text-sm font-semibold text-slate-200">"Email address"</h3>
                    <p class="mt-1 text-sm text-slate-400">{current_email.get_value()}</p>
                    <p class="mt-1 text-xs text-slate-500">
                        "Only a site admin can change an account's email address."
                    </p>
                </section>
            }
            .into_any();
        }

        view! {
            <section class="mt-4 border-t border-slate-800 pt-4">
                <h3 class="text-sm font-semibold text-slate-200">"Email address"</h3>
                <p class="mt-1 text-sm text-slate-300">{current_email.get_value()}</p>
                <p class="mt-1 text-xs text-slate-500">
                    "This is the address they sign in with. Changing it does not change their password, and both the old and new addresses are notified."
                </p>
                <Show
                    when=move || email_editing.get()
                    fallback=move || view! {
                        <button
                            type="button"
                            on:click=move |_| {
                                email_feedback.set(None);
                                email_editing.set(true);
                            }
                            class="mt-3 rounded-full border border-slate-700 px-3 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800"
                        >
                            "Change email address"
                        </button>
                    }
                >
                    <div class="mt-3 space-y-3 rounded-lg border border-slate-800 bg-slate-950/60 p-3">
                        <div>
                            <label
                                class="block text-xs font-medium text-slate-400"
                                for=email_input_id.get_value()
                            >
                                "New email address"
                            </label>
                            <input
                                id=email_input_id.get_value()
                                type="email"
                                maxlength="254"
                                autocomplete="off"
                                class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                                placeholder="name@example.org"
                                prop:disabled=move || email_busy.get()
                                prop:value=move || email_value.get()
                                on:input=move |event| email_value.set(event_target_value(&event))
                            />
                        </div>
                        <div>
                            <label
                                class="block text-xs font-medium text-slate-400"
                                for=email_confirm_id.get_value()
                            >
                                "Confirm new email address"
                            </label>
                            <input
                                id=email_confirm_id.get_value()
                                type="email"
                                maxlength="254"
                                autocomplete="off"
                                class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                                placeholder="Type it again"
                                prop:disabled=move || email_busy.get()
                                prop:value=move || email_confirm.get()
                                on:input=move |event| email_confirm.set(event_target_value(&event))
                            />
                        </div>
                        <div class="flex flex-wrap gap-2">
                            <button
                                type="button"
                                on:click=apply_email_change
                                prop:disabled=move || email_busy.get()
                                class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                            >
                                {move || if email_busy.get() { "Saving…" } else { "Save email address" }}
                            </button>
                            <button
                                type="button"
                                on:click=move |_| {
                                    email_editing.set(false);
                                    email_value.set(String::new());
                                    email_confirm.set(String::new());
                                }
                                prop:disabled=move || email_busy.get()
                                class="rounded-full border border-slate-700 px-3 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                            >
                                "Cancel"
                            </button>
                        </div>
                    </div>
                </Show>
                <Show when=move || email_feedback.get().is_some()>
                    {move || email_feedback.get().map(|feedback| match feedback {
                        Ok(message) => view! {
                            <p class="mt-3 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300" aria-live="polite">
                                {message}
                            </p>
                        }.into_any(),
                        Err(message) => view! {
                            <p class="mt-3 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300" role="alert">
                                {message}
                            </p>
                        }.into_any(),
                    })}
                </Show>
            </section>
        }
        .into_any()
    };

    let account_status_section = move || {
        let deactivation = deactivation.clone();
        let detail = deactivation.map(|d| {
            let restores_to = d.previous_role.label();
            let reason = (!d.reason.is_empty()).then(|| {
                view! {
                    <p class="mt-1 text-sm text-slate-300">"“" {d.reason} "”"</p>
                }
            });
            view! {
                <div class="mt-3 rounded-lg border border-slate-800 bg-slate-950/60 px-3 py-2">
                    <p class="text-xs text-slate-500">
                        "Deactivated by " {d.by} " on " {d.at}
                        " · reactivating restores the " {restores_to} " role."
                    </p>
                    {reason}
                </div>
            }
        });

        let controls = if !is_site_admin {
            let message = if is_deactivated {
                "Only a site admin can reactivate an account."
            } else {
                "Only a site admin can deactivate an account."
            };
            view! { <p class="mt-4 text-sm text-slate-500">{message}</p> }.into_any()
        } else if is_self_account {
            view! {
                <p class="mt-4 text-sm text-slate-500">
                    "You cannot deactivate your own account."
                </p>
            }
            .into_any()
        } else if is_deactivated {
            view! {
                <div class="mt-4">
                    <button
                        type="button"
                        on:click=reactivate_account
                        prop:disabled=move || status_busy.get()
                        class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        {move || if status_busy.get() { "Reactivating…" } else { "Reactivate account" }}
                    </button>
                </div>
            }
            .into_any()
        } else {
            view! {
                <div class="mt-4">
                    <Show
                        when=move || status_confirming.get()
                        fallback=move || view! {
                            <button
                                type="button"
                                on:click=move |_| status_confirming.set(true)
                                class="rounded-full border border-rose-500/50 px-3 py-2 text-sm font-semibold text-rose-300 hover:bg-rose-500/10"
                            >
                                "Deactivate account"
                            </button>
                        }
                    >
                        <div class="space-y-3 rounded-lg border border-rose-500/30 bg-rose-500/5 p-3">
                            <p class="text-sm text-slate-300">
                                "They will be signed out and will no longer appear in lists or pickers. Nothing is deleted, and you can reactivate them later."
                            </p>
                            <div>
                                <label
                                    class="block text-xs font-medium text-slate-400"
                                    for=status_reason_id.get_value()
                                >
                                    "Reason (optional)"
                                </label>
                                <textarea
                                    id=status_reason_id.get_value()
                                    rows="2"
                                    maxlength="1000"
                                    class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                                    placeholder="e.g. Duplicate account — merged into their other login"
                                    prop:disabled=move || status_busy.get()
                                    prop:value=move || status_reason.get()
                                    on:input=move |event| status_reason.set(event_target_value(&event))
                                ></textarea>
                            </div>
                            <div class="flex flex-wrap gap-2">
                                <button
                                    type="button"
                                    on:click=deactivate_account
                                    prop:disabled=move || status_busy.get()
                                    class="rounded-full bg-rose-500 px-3 py-2 text-sm font-semibold text-white hover:bg-rose-600 disabled:opacity-50"
                                >
                                    {move || if status_busy.get() { "Deactivating…" } else { "Confirm deactivation" }}
                                </button>
                                <button
                                    type="button"
                                    on:click=move |_| status_confirming.set(false)
                                    prop:disabled=move || status_busy.get()
                                    class="rounded-full border border-slate-700 px-3 py-2 text-sm font-medium text-slate-200 hover:bg-slate-800 disabled:opacity-50"
                                >
                                    "Cancel"
                                </button>
                            </div>
                        </div>
                    </Show>
                </div>
            }
            .into_any()
        };

        view! {
            <section class="mt-4 border-t border-slate-800 pt-4">
                <div class="flex flex-wrap items-center gap-2">
                    <h3 class="text-sm font-semibold text-slate-200">"Account status"</h3>
                    <span class=badge(if is_deactivated {
                        "bg-slate-700/40 text-slate-400 ring-1 ring-slate-600"
                    } else {
                        "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
                    })>{if is_deactivated { "Deactivated" } else { "Active" }}</span>
                </div>
                <p class="mt-1 text-xs text-slate-500">
                    "A deactivated account cannot sign in and is hidden from lists and pickers. Its role, case access, and history are kept so reactivating restores everything."
                </p>
                {detail}
                {controls}
                <Show when=move || status_feedback.get().is_some()>
                    {move || status_feedback.get().map(|feedback| match feedback {
                        Ok(message) => view! {
                            <p class="mt-3 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300" aria-live="polite">
                                {message}
                            </p>
                        }.into_any(),
                        Err(message) => view! {
                            <p class="mt-3 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300" role="alert">
                                {message}
                            </p>
                        }.into_any(),
                    })}
                </Show>
            </section>
        }
        .into_any()
    };

    let account_role_section = move || {
        let helper = if is_deactivated {
            "Reactivate this account before changing its role."
        } else if is_site_admin {
            "Site admins apply account-role changes directly."
        } else if can_request_role_change {
            "Operations admins can request account-role changes for other users."
        } else {
            "Only a site admin can change your own account role."
        };

        let controls = if is_deactivated {
            // The role select would show a role this account does not currently
            // hold, and applying it would silently un-retire them.
            view! {
                <p class="mt-4 text-sm text-slate-500">
                    "This account is deactivated. Reactivate it under Account status to change its role."
                </p>
            }
            .into_any()
        } else if is_site_admin {
            view! {
                <div class="mt-4 flex flex-col gap-3 sm:flex-row sm:items-end">
                    <div class="sm:min-w-[14rem]">
                        <label
                            class="block text-xs font-medium text-slate-400"
                            for=role_select_id.get_value()
                        >
                            "Choose account role"
                        </label>
                        <select
                            id=role_select_id.get_value()
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100"
                            prop:disabled=move || role_busy.get()
                            prop:value=move || selected_role.get()
                            on:change=move |event| selected_role.set(event_target_value(&event))
                        >
                            {AccountRole::ASSIGNABLE
                                .into_iter()
                                .map(|role| {
                                    view! { <option value=role.slug()>{role.label()}</option> }
                                })
                                .collect_view()}
                        </select>
                    </div>
                    <button
                        type="button"
                        on:click=apply_role_change
                        prop:disabled=move || {
                            role_busy.get() || selected_role.get() == current_role.slug()
                        }
                        class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        {move || if role_busy.get() { "Applying…" } else { "Apply role" }}
                    </button>
                </div>
            }
            .into_any()
        } else if can_request_role_change {
            view! {
                <div class="mt-4 space-y-3">
                    <div class="sm:max-w-xs">
                        <label
                            class="block text-xs font-medium text-slate-400"
                            for=role_select_id.get_value()
                        >
                            "Requested account role"
                        </label>
                        <select
                            id=role_select_id.get_value()
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100"
                            prop:disabled=move || role_busy.get()
                            prop:value=move || selected_role.get()
                            on:change=move |event| selected_role.set(event_target_value(&event))
                        >
                            {AccountRole::ASSIGNABLE
                                .into_iter()
                                .map(|role| {
                                    view! { <option value=role.slug()>{role.label()}</option> }
                                })
                                .collect_view()}
                        </select>
                    </div>
                    <div>
                        <label
                            class="block text-xs font-medium text-slate-400"
                            for=role_note_id.get_value()
                        >
                            "Request note"
                        </label>
                        <textarea
                            id=role_note_id.get_value()
                            rows="2"
                            maxlength="1000"
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                            placeholder="Optional context for the site admin"
                            prop:disabled=move || role_busy.get()
                            prop:value=move || role_note.get()
                            on:input=move |event| role_note.set(event_target_value(&event))
                        ></textarea>
                    </div>
                    <button
                        type="button"
                        on:click=apply_role_change
                        prop:disabled=move || {
                            role_busy.get() || selected_role.get() == current_role.slug()
                        }
                        class="rounded-full border border-primary-500/50 px-3 py-2 text-sm font-semibold text-primary-300 hover:bg-primary-500/10 disabled:opacity-50"
                    >
                        {move || if role_busy.get() { "Submitting…" } else { "Request role" }}
                    </button>
                </div>
            }
            .into_any()
        } else {
            view! {
                <p class="mt-4 text-sm text-slate-500">
                    "Your account role is visible here, but only a site admin can change it."
                </p>
            }
            .into_any()
        };

        view! {
            <section class="mt-4 border-t border-slate-800 pt-4">
                <div class="flex flex-wrap items-center gap-2">
                    <h3 class="text-sm font-semibold text-slate-200">"Account role"</h3>
                    <span class=badge(current_role.badge_classes())>{current_role.label()}</span>
                </div>
                <p class="mt-1 text-xs text-slate-500">{helper}</p>
                <Show when=move || role_feedback.get().is_some()>
                    {move || {
                        role_feedback.get().map(|feedback| match feedback {
                            Ok(message) => {
                                view! {
                                    <p
                                        class="mt-3 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300"
                                        aria-live="polite"
                                    >
                                        {message}
                                    </p>
                                }
                                    .into_any()
                            }
                            Err(message) => {
                                view! {
                                    <p
                                        class="mt-3 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300"
                                        role="alert"
                                    >
                                        {message}
                                    </p>
                                }
                                    .into_any()
                            }
                        })
                    }}
                </Show>
                {controls}
            </section>
        }
        .into_any()
    };

    let information_access_section = move || {
        let status = if current_information_access {
            "Granted"
        } else {
            "Denied"
        };
        view! {
            <section class="mt-4 border-t border-slate-800 pt-4">
                <div class="flex flex-wrap items-center gap-2">
                    <h3 class="text-sm font-semibold text-slate-200">"Information access"</h3>
                    <span class=badge(if current_information_access {
                        "bg-emerald-500/15 text-emerald-300 ring-1 ring-emerald-500/30"
                    } else {
                        "bg-slate-700/40 text-slate-300 ring-1 ring-slate-600"
                    })>{status}</span>
                </div>
                <p class="mt-1 text-xs text-slate-500">
                    "This grant enables Contacts, Organizations, and Funding management. It does not grant Admin dashboard or case permissions, and it never gives a client access."
                </p>
                {if is_deactivated {
                    // Consistent with the role section: a retired account's grants are
                    // frozen until it is restored, so they cannot drift while it
                    // is out of service.
                    view! {
                        <p class="mt-4 text-sm text-slate-500">
                            "This account is deactivated. Reactivate it under Account status to change this grant."
                        </p>
                    }
                    .into_any()
                } else if is_site_admin {
                    view! {
                        <div class="mt-4 space-y-3">
                            <label class="flex items-start gap-3 text-sm text-slate-200">
                                <input
                                    type="checkbox"
                                    class="mt-0.5 h-4 w-4 rounded border-slate-600 bg-slate-950"
                                    prop:checked=move || information_enabled.get()
                                    prop:disabled=move || information_busy.get()
                                    on:change=move |event| information_enabled.set(event_target_checked(&event))
                                />
                                <span>"Can manage contacts, organizations, and funding information."</span>
                            </label>
                            <button
                                type="button"
                                on:click=apply_information_access
                                prop:disabled=move || {
                                    information_busy.get()
                                        || information_enabled.get() == current_information_access
                                }
                                class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                            >
                                {move || if information_busy.get() { "Saving…" } else { "Save access" }}
                            </button>
                        </div>
                    }
                    .into_any()
                } else if !current_information_access && current_role.has_volunteer_privileges() {
                    view! {
                        <div class="mt-4 space-y-3">
                            <p class="text-sm text-slate-500">
                                "Operations admins can request this grant for site-admin approval."
                            </p>
                            <div>
                                <label
                                    class="block text-xs font-medium text-slate-400"
                                    for=information_note_id.get_value()
                                >
                                    "Request note"
                                </label>
                                <textarea
                                    id=information_note_id.get_value()
                                    rows="2"
                                    maxlength="1000"
                                    class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                                    placeholder="Optional context for the site admin"
                                    prop:disabled=move || information_busy.get()
                                    prop:value=move || information_request_note.get()
                                    on:input=move |event| information_request_note.set(event_target_value(&event))
                                ></textarea>
                            </div>
                            <button
                                type="button"
                                on:click=request_information_access
                                prop:disabled=move || information_busy.get()
                                class="rounded-full border border-primary-500/50 px-3 py-2 text-sm font-semibold text-primary-300 hover:bg-primary-500/10 disabled:opacity-50"
                            >
                                {move || if information_busy.get() { "Submitting…" } else { "Request access" }}
                            </button>
                        </div>
                    }
                    .into_any()
                } else {
                    let message = if current_role.has_volunteer_privileges() {
                        "This access is already granted. Only a site admin can revoke it."
                    } else {
                        "Client accounts are not eligible for information access."
                    };
                    view! { <p class="mt-4 text-sm text-slate-500">{message}</p> }.into_any()
                }}
                <Show when=move || information_feedback.get().is_some()>
                    {move || information_feedback.get().map(|feedback| match feedback {
                        Ok(message) => view! {
                            <p class="mt-3 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300" aria-live="polite">
                                {message}
                            </p>
                        }.into_any(),
                        Err(message) => view! {
                            <p class="mt-3 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300" role="alert">
                                {message}
                            </p>
                        }.into_any(),
                    })}
                </Show>
            </section>
        }
        .into_any()
    };

    let case_access_section = move || {
        // A site admin holds every capability on every case, so per-case
        // assignments are meaningless for this account. A deactivated account
        // never reaches this branch: deactivating moves the role off
        // `site_admin`, so its preserved assignments still show below.
        if current_role.has_full_case_access() {
            return view! {
                <section class="mt-4 border-t border-slate-800 pt-4">
                    <h3 class="text-sm font-semibold text-slate-200">"Case access"</h3>
                    <p class="mt-3 text-sm text-slate-400">
                        "Site admins have full access to every case. Per-case assignments do not apply to this account."
                    </p>
                </section>
            }
            .into_any();
        }
        let helper = if is_deactivated {
            "Case access is preserved while the account is deactivated, and is restored with it."
        } else if is_site_admin {
            "Site admins apply case-access changes directly."
        } else if can_edit_capabilities_directly {
            "You can edit your own case access directly."
        } else {
            "Operations admins can draft changes here and submit them for site-admin approval."
        };

        // A retired account's assignments are shown read-only: they survive
        // deactivation so the restore is lossless, and editing them here would
        // change access for an account that cannot use it.
        let header_buttons = if is_deactivated {
            ().into_any()
        } else if editing.get() {
            view! {
                <button
                    type="button"
                    on:click=save_edit
                    prop:disabled=move || saving.get()
                    class="rounded-full bg-primary-500 px-3 py-1.5 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                >
                    {move || if saving.get() {
                        if can_edit_capabilities_directly {
                            "Saving…"
                        } else {
                            "Submitting…"
                        }
                    } else if can_edit_capabilities_directly {
                        "Save"
                    } else {
                        "Submit request"
                    }}
                </button>
                <button
                    type="button"
                    on:click=cancel_edit
                    class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-300 hover:bg-slate-800"
                >
                    "Cancel"
                </button>
            }
            .into_any()
        } else {
            view! {
                <button
                    type="button"
                    on:click=begin_edit
                    class="rounded-full border border-slate-700 px-3 py-1.5 text-sm font-medium text-slate-200 hover:bg-slate-800"
                >
                    "Edit"
                </button>
            }
            .into_any()
        };

        let body = if editing.get() {
            let add_control = view! {
                <div class="mt-4 grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(12rem,auto)_auto] sm:items-end">
                    <div class="relative">
                        <label
                            class="block text-xs font-medium text-slate-400"
                            for=case_search_id.get_value()
                        >
                            "Search for a case"
                        </label>
                        <input
                            id=case_search_id.get_value()
                            type="search"
                            role="combobox"
                            aria-autocomplete="list"
                            aria-expanded=move || picker_open.get().to_string()
                            aria-controls=case_results_id.get_value()
                            aria-activedescendant=move || {
                                active_case_result
                                    .get()
                                    .map(|index| format!("{}-{index}", case_results_id.get_value()))
                            }
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                            placeholder="Case name or ID"
                            prop:value=move || {
                                let label = new_case_label.get();
                                if label.is_empty() { case_query.get() } else { label }
                            }
                            on:focus=move |_| {
                                picker_open.set(true);
                                active_case_result.set(None);
                                new_case.set(String::new());
                                new_case_label.set(String::new());
                            }
                            on:input=move |event| {
                                new_case.set(String::new());
                                new_case_label.set(String::new());
                                picker_open.set(true);
                                active_case_result.set(None);
                                case_query.set(event_target_value(&event));
                            }
                            on:keydown=move |event: leptos::ev::KeyboardEvent| {
                                let count = case_results.with(Vec::len);
                                match event.key().as_str() {
                                    "ArrowDown" if count > 0 => {
                                        event.prevent_default();
                                        active_case_result.update(|active| {
                                            *active = Some(active.map_or(0, |index| (index + 1).min(count - 1)));
                                        });
                                    }
                                    "ArrowUp" if count > 0 => {
                                        event.prevent_default();
                                        active_case_result.update(|active| {
                                            *active = Some(active.map_or(count - 1, |index| index.saturating_sub(1)));
                                        });
                                    }
                                    "Enter" => {
                                        if let Some(index) = active_case_result.get_untracked() {
                                            event.prevent_default();
                                            if let Some(case) = case_results
                                                .get_untracked()
                                                .get(index)
                                                .cloned()
                                            {
                                                new_case.set(case.id);
                                                new_case_label.set(case.name);
                                                picker_open.set(false);
                                                active_case_result.set(None);
                                            }
                                        }
                                    }
                                    "Escape" => {
                                        picker_open.set(false);
                                        active_case_result.set(None);
                                    }
                                    _ => {}
                                }
                            }
                        />
                        {case_result_list}
                    </div>
                    <div>
                        <label
                            class="block text-xs font-medium text-slate-400"
                            for=case_preset_id.get_value()
                        >
                            "Access preset"
                        </label>
                        <select
                            id=case_preset_id.get_value()
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100"
                            prop:value=move || new_preset.get()
                            on:change=move |event| new_preset.set(event_target_value(&event))
                        >
                            {CasePreset::ALL
                                .into_iter()
                                .map(|preset| {
                                    view! { <option value=preset.slug()>{preset.label()}</option> }
                                })
                                .collect_view()}
                        </select>
                    </div>
                    <button
                        type="button"
                        on:click=add_to_draft
                        prop:disabled=move || new_case.get().is_empty()
                        class="rounded-full bg-primary-500 px-3 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50"
                    >
                        "Add"
                    </button>
                </div>
            };
            let request_note = (!can_edit_capabilities_directly).then(|| {
                view! {
                    <div class="mt-4">
                        <label
                            class="block text-xs font-medium text-slate-400"
                            for=capability_note_id.get_value()
                        >
                            "Request note"
                        </label>
                        <textarea
                            id=capability_note_id.get_value()
                            rows="2"
                            maxlength="1000"
                            class="mt-1 w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500"
                            placeholder="Optional context for the site admin"
                            prop:value=move || capability_request_note.get()
                            on:input=move |event| capability_request_note.set(event_target_value(&event))
                        ></textarea>
                    </div>
                }
            });

            view! {
                <Show
                    when=move || !draft.get().is_empty()
                    fallback=|| view! { <p class="text-sm text-slate-500">"No case assignments."</p> }
                >
                    <div>
                        <For each=move || draft.get() key=|row| row.case_id.clone() let:row>
                            {
                                let row_caps = row.caps;
                                let row_removed = row.removed;
                                let case_name = row.name.clone();
                                let checkboxes = CaseCapability::ALL
                                    .into_iter()
                                    .map(|capability| {
                                        let unmet = move || {
                                            let held = row_caps.get();
                                            capability.requires().iter().any(|required| !held.contains(required))
                                        };
                                        let title = move || {
                                            let held = row_caps.get();
                                            match capability
                                                .requires()
                                                .iter()
                                                .find(|required| !held.contains(required))
                                            {
                                                Some(missing) => {
                                                    format!("Requires \"{}\" first.", missing.label())
                                                }
                                                None => capability.label().to_string(),
                                            }
                                        };
                                        view! {
                                            <label
                                                class="inline-flex items-center gap-1.5 text-xs"
                                                class=("text-slate-300", move || !unmet())
                                                class=("text-slate-600", unmet)
                                                title=title
                                            >
                                                <input
                                                    type="checkbox"
                                                    class="h-3.5 w-3.5 rounded border-slate-600 bg-slate-950"
                                                    prop:checked=move || row_caps.get().contains(&capability)
                                                    prop:disabled=move || row_removed.get() || unmet()
                                                    on:change=move |event| {
                                                        let enabled = event_target_checked(&event);
                                                        row_caps.update(|caps| {
                                                            if enabled {
                                                                if !caps.contains(&capability) {
                                                                    caps.push(capability);
                                                                }
                                                            } else {
                                                                caps.retain(|held| *held != capability);
                                                                loop {
                                                                    let held = caps.clone();
                                                                    let before = caps.len();
                                                                    caps.retain(|held_capability| {
                                                                        held_capability
                                                                            .requires()
                                                                            .iter()
                                                                            .all(|required| held.contains(required))
                                                                    });
                                                                    if caps.len() == before {
                                                                        break;
                                                                    }
                                                                }
                                                            }
                                                        });
                                                    }
                                                />
                                                {capability.label()}
                                            </label>
                                        }
                                    })
                                    .collect_view();
                                view! {
                                    <div class=move || {
                                        let base = "border-b border-slate-800 py-3 last:border-b-0";
                                        if row_removed.get() {
                                            format!("{base} opacity-50")
                                        } else {
                                            base.to_string()
                                        }
                                    }>
                                        <div class="flex flex-wrap items-center justify-between gap-2">
                                            <span class="text-sm font-medium text-slate-200">
                                                {case_name.clone()}
                                            </span>
                                            <button
                                                type="button"
                                                on:click=move |_| row_removed.update(|removed| *removed = !*removed)
                                                class="rounded-full border border-rose-500/40 px-2 py-1 text-xs font-medium text-rose-300 hover:bg-rose-500/10"
                                            >
                                                {move || if row_removed.get() { "Undo remove" } else { "Remove" }}
                                            </button>
                                        </div>
                                        <div class="mt-2 flex flex-wrap gap-x-4 gap-y-1.5">
                                            {checkboxes}
                                        </div>
                                    </div>
                                }
                            }
                        </For>
                    </div>
                </Show>
                {add_control}
                {request_note}
            }
            .into_any()
        } else {
            let assignments = originals.get_value();
            if assignments.is_empty() {
                view! { <p class="text-sm text-slate-500">"No case assignments."</p> }.into_any()
            } else {
                assignments
                    .into_iter()
                    .map(|assignment| {
                        let name = case_name(&assignment.case_id);
                        let capabilities = assignment.capabilities.clone();
                        let capability_badges = if capabilities.is_empty() {
                            view! {
                                <span class="text-xs text-slate-500">"No capabilities"</span>
                            }
                            .into_any()
                        } else {
                            capabilities
                                .into_iter()
                                .map(|capability| {
                                    view! {
                                        <span class="rounded-full bg-slate-800 px-2 py-0.5 text-xs text-slate-300">
                                            {capability.label()}
                                        </span>
                                    }
                                    .into_any()
                                })
                                .collect_view()
                                .into_any()
                        };
                        view! {
                            <div class="border-b border-slate-800 py-3 last:border-b-0">
                                <span class="text-sm font-medium text-slate-200">{name}</span>
                                <div class="mt-2 flex flex-wrap gap-1.5">{capability_badges}</div>
                            </div>
                        }
                        .into_any()
                    })
                    .collect_view()
                    .into_any()
            }
        };

        view! {
            <section class="mt-4 border-t border-slate-800 pt-4">
                <div class="flex flex-wrap items-start justify-between gap-3">
                    <div>
                        <h3 class="text-sm font-semibold text-slate-200">"Case access"</h3>
                        <p class="mt-1 text-xs text-slate-500">{helper}</p>
                    </div>
                    <div class="flex flex-wrap items-center gap-2">{header_buttons}</div>
                </div>
                <Show when=move || case_feedback.get().is_some()>
                    {move || {
                        case_feedback.get().map(|feedback| match feedback {
                            Ok(message) => {
                                view! {
                                    <p
                                        class="mt-3 rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300"
                                        aria-live="polite"
                                    >
                                        {message}
                                    </p>
                                }
                                    .into_any()
                            }
                            Err(message) => {
                                view! {
                                    <p
                                        class="mt-3 rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300"
                                        role="alert"
                                    >
                                        {message}
                                    </p>
                                }
                                    .into_any()
                            }
                        })
                    }}
                </Show>
                <div class="mt-3">{body}</div>
            </section>
        }
        .into_any()
    };

    let log_open = RwSignal::new(false);
    let log_user_id = StoredValue::new(user_id.clone());
    let log_section_id = StoredValue::new(format!("user-change-log-{user_id}"));

    view! {
        <div class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5">
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <p class="font-medium text-slate-100">{full_name}</p>
                    <p class="text-xs text-slate-500">{email}</p>
                </div>
                <span class=badge(current_role.badge_classes())>{current_role.label()}</span>
            </div>

            {account_role_section}
            {email_section}
            {account_status_section}
            {information_access_section}
            {case_access_section}

            {is_site_admin.then(|| view! {
                <section class="mt-4 border-t border-slate-800 pt-4">
                    <div class="flex flex-wrap items-center justify-between gap-3">
                        <div>
                            <h3 class="text-sm font-semibold text-slate-200">"Change log"</h3>
                            <p class="mt-1 text-xs text-slate-500">
                                "Audited changes for this user."
                            </p>
                        </div>
                        <button
                            type="button"
                            on:click=move |_| log_open.update(|open| *open = !*open)
                            aria-expanded=move || log_open.get().to_string()
                            aria-controls=log_section_id.get_value()
                            class="rounded-full border border-slate-700 px-2 py-1 text-xs font-medium text-slate-300 hover:bg-slate-800"
                        >
                            {move || if log_open.get() { "Hide" } else { "Open change log" }}
                        </button>
                    </div>
                    <Show when=move || log_open.get()>
                        <div id=log_section_id.get_value() class="mt-3">
                            <ChangeLog
                                scope=AuditScope::User
                                entity_id=log_user_id.get_value()
                            />
                        </div>
                    </Show>
                </section>
            })}
        </div>
    }
    .into_any()
}
