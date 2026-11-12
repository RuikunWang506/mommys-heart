//! Categories and rules: the page that owns contact classification.
//!
//! Two halves. The taxonomy manager edits the categories themselves — add,
//! rename, reorder, delete, and paste a whole list of subcategories at once.
//! The rules manager decides what gets *offered* for a given kind of contact,
//! which is what turns a flat list of 26 vendor categories into "pick one
//! location, then pick any number of vendor types".

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;

use crate::components::guard::require_information_management_access;
use crate::components::layout::Layout;
use crate::server_fns::contact_directory::{
    add_contact_category, add_contact_subcategories, delete_contact_category,
    list_contact_categories, rename_contact_category, reorder_contact_categories, ContactCategory,
};
use crate::server_fns::contact_rules::{
    delete_contact_rule, list_contact_rules, save_contact_rule, ContactRule, RuleField,
    RuleGroupInput, RuleInput, RuleTrigger,
};
use crate::server_fns::contacts::ContactType;
use crate::server_fns::err_text;
use crate::state::AppState;

const INPUT: &str = "w-full rounded-xl border border-slate-700 bg-slate-900 px-3 py-2 text-sm text-slate-100 placeholder:text-slate-500 focus:border-primary-500 focus:outline-none focus:ring-2 focus:ring-primary-500/40";
const LABEL: &str = "mb-1 block text-xs font-medium text-slate-400";
const PANEL: &str = "rounded-card border border-slate-800 bg-slate-900 shadow-card p-5";
const BTN: &str = "rounded-full border border-slate-700 px-3 py-1.5 text-xs font-medium text-slate-300 hover:bg-slate-800 disabled:opacity-50";
const PRIMARY_BTN: &str = "rounded-full bg-primary-500 px-4 py-2 text-sm font-semibold text-white hover:bg-primary-600 disabled:opacity-50";

/// "1 subcategory" / "3 subcategories", so counts read as English.
fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

#[component]
pub fn ContactCategoriesPage() -> impl IntoView {
    let state = expect_context::<AppState>();
    require_information_management_access(state, move || {
        let can_edit = state.has_operations_admin_permissions();
        view! {
            <Layout title="Categories and rules".to_string()>
                <div class="mb-6">
                    <A href="/contacts" attr:class="text-sm text-primary-400 hover:text-primary-300">
                        "\u{2190} Back to contacts"
                    </A>
                    <p class="mt-2 max-w-3xl text-sm text-slate-400">
                        "Categories classify contacts. Rules decide which categories and fields are \
                         offered for a kind of contact \u{2014} so marking someone a vendor can ask \
                         for one location and any number of vendor types, without anyone editing code."
                    </p>
                </div>
                <Show
                    when=move || can_edit
                    fallback=|| view! {
                        <p class="rounded-card border border-slate-800 bg-slate-900 shadow-card p-5 text-sm text-slate-400">
                            "Categories and rules are managed by administrators. You can see them on \
                             each contact."
                        </p>
                    }
                >
                    <ClassificationManager />
                </Show>
            </Layout>
        }
        .into_any()
    })
}

#[component]
fn ClassificationManager() -> impl IntoView {
    let categories = RwSignal::new(Vec::<ContactCategory>::new());
    let rules = RwSignal::new(Vec::<ContactRule>::new());
    let error = RwSignal::new(String::new());
    let notice = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    Effect::new(move |_| {
        spawn_local(async move {
            match list_contact_categories().await {
                Ok(items) => {
                    categories.try_set(items);
                }
                Err(e) => {
                    error.try_set(err_text(e));
                }
            }
            if let Ok(items) = list_contact_rules().await {
                rules.try_set(items);
            }
        });
    });

    view! {
        <div class="space-y-5">
            <Show when=move || !error.get().is_empty()>
                <p class="rounded-lg border border-rose-500/40 bg-rose-500/10 px-3 py-2 text-sm text-rose-300" role="alert">
                    {move || error.get()}
                </p>
            </Show>
            <Show when=move || !notice.get().is_empty()>
                <p class="rounded-lg border border-emerald-500/40 bg-emerald-500/10 px-3 py-2 text-sm text-emerald-300">
                    {move || notice.get()}
                </p>
            </Show>

            <TaxonomyManager categories busy error notice />
            <RulesManager categories rules busy error notice />
        </div>
    }
}

/// Add, rename, reorder and delete the categories themselves.
#[component]
fn TaxonomyManager(
    categories: RwSignal<Vec<ContactCategory>>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    notice: RwSignal<String>,
) -> impl IntoView {
    let new_name = RwSignal::new(String::new());
    let new_parent = RwSignal::new(String::new());
    let paste_parent = RwSignal::new(String::new());
    let paste_body = RwSignal::new(String::new());
    let renaming = RwSignal::new(None::<String>);
    let rename_value = RwSignal::new(String::new());

    let roots = move || {
        categories
            .get()
            .into_iter()
            .filter(|category| category.parent_id.is_none())
            .collect::<Vec<_>>()
    };

    let run = move |work: std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ContactCategory>, ServerFnError>>>>| {
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            match work.await {
                Ok(items) => {
                    categories.try_set(items);
                }
                Err(e) => {
                    error.try_set(err_text(e));
                }
            }
            busy.try_set(false);
        });
    };

    let add = move |_| {
        let name = new_name.get_untracked();
        let parent = new_parent.get_untracked();
        new_name.set(String::new());
        run(Box::pin(async move {
            let parent_id = (!parent.is_empty()).then_some(parent);
            add_contact_category(name, parent_id).await
        }));
    };

    let paste = move |_| {
        let parent = paste_parent.get_untracked();
        let body = paste_body.get_untracked();
        if parent.is_empty() {
            error.set("Choose which category the pasted list belongs under.".into());
            return;
        }
        paste_body.set(String::new());
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            match add_contact_subcategories(parent, body).await {
                Ok(result) => {
                    categories.try_set(result.categories);
                    // The server counts what it actually inserted: names that
                    // already existed are skipped, and saying so avoids
                    // claiming additions that did not happen.
                    let mut message =
                        format!("Added {}.", plural(result.added, "subcategory", "subcategories"));
                    if result.skipped > 0 {
                        message.push_str(&format!(
                            " Skipped {} that already existed.",
                            result.skipped
                        ));
                    }
                    notice.try_set(message);
                }
                Err(e) => {
                    error.try_set(err_text(e));
                }
            }
            busy.try_set(false);
        });
    };

    // One level's ids in display order, so a move can be stored as a whole list.
    let sibling_ids = move |category: &ContactCategory| -> Vec<String> {
        categories
            .get_untracked()
            .into_iter()
            .filter(|item| item.parent_id == category.parent_id)
            .map(|item| item.id)
            .collect()
    };

    let move_by = move |category: ContactCategory, delta: i32| {
        let mut ids = sibling_ids(&category);
        let Some(index) = ids.iter().position(|id| id == &category.id) else {
            return;
        };
        let target = index as i32 + delta;
        if target < 0 || target as usize >= ids.len() {
            return;
        }
        ids.swap(index, target as usize);
        run(Box::pin(
            async move { reorder_contact_categories(ids).await },
        ));
    };

    let save_rename = move |category_id: String| {
        let name = rename_value.get_untracked();
        renaming.set(None);
        run(Box::pin(async move {
            rename_contact_category(category_id, name).await
        }));
    };

    let remove = move |category_id: String| {
        run(Box::pin(async move {
            delete_contact_category(category_id).await
        }));
    };

    // A category and its children, rendered as one block.
    let tree = move || {
        let all = categories.get();
        all.iter()
            .filter(|category| category.parent_id.is_none())
            .map(|root| {
                let children: Vec<ContactCategory> = all
                    .iter()
                    .filter(|item| item.parent_id.as_deref() == Some(root.id.as_str()))
                    .cloned()
                    .collect();
                let root = root.clone();
                view! {
                    <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
                        <CategoryRow
                            category=root.clone()
                            is_root=true
                            busy
                            renaming
                            rename_value
                            on_move=Callback::new(move |(category, delta)| move_by(category, delta))
                            on_rename=Callback::new(move |id| save_rename(id))
                            on_delete=Callback::new(move |id| remove(id))
                        />
                        <div class="mt-1 space-y-0.5 border-l border-slate-800 pl-4">
                            {children
                                .into_iter()
                                .map(|child| view! {
                                    <CategoryRow
                                        category=child
                                        is_root=false
                                        busy
                                        renaming
                                        rename_value
                                        on_move=Callback::new(move |(category, delta)| move_by(category, delta))
                                        on_rename=Callback::new(move |id| save_rename(id))
                                        on_delete=Callback::new(move |id| remove(id))
                                    />
                                })
                                .collect_view()}
                        </div>
                    </div>
                }
            })
            .collect_view()
    };

    view! {
        <section class=PANEL>
            <h2 class="text-sm font-semibold text-slate-200">"Categories"</h2>
            <p class="mt-1 text-xs text-slate-500">
                "Two levels: a category and its subcategories. \u{201C}/\u{201D} is reserved for \
                 showing a subcategory beneath its parent, so it cannot appear in a name."
            </p>

            <div class="mt-4 grid gap-3 sm:grid-cols-[1fr_1fr_auto]">
                <input
                    class=INPUT
                    placeholder="New category or subcategory name"
                    prop:value=move || new_name.get()
                    on:input=move |event| new_name.set(event_target_value(&event))
                />
                <select
                    class=INPUT
                    prop:value=move || new_parent.get()
                    on:change=move |event| new_parent.set(event_target_value(&event))
                >
                    <option value="">"As a top-level category"</option>
                    {move || roots()
                        .into_iter()
                        .map(|category| view! {
                            <option value=category.id>{format!("Under {}", category.name)}</option>
                        })
                        .collect_view()}
                </select>
                <button type="button" on:click=add prop:disabled=move || busy.get() class=PRIMARY_BTN>
                    "Add"
                </button>
            </div>

            <details class="mt-4 rounded-lg border border-slate-800 bg-slate-950">
                <summary class="cursor-pointer px-3 py-2 text-xs font-medium text-slate-300">
                    "Paste a list of subcategories"
                </summary>
                <div class="space-y-3 border-t border-slate-800 p-3">
                    <p class="text-xs text-slate-500">
                        "One name per line. Names that already exist are skipped, so a corrected \
                         list can be pasted again safely."
                    </p>
                    <select
                        class=INPUT
                        prop:value=move || paste_parent.get()
                        on:change=move |event| paste_parent.set(event_target_value(&event))
                    >
                        <option value="">"Choose the parent category\u{2026}"</option>
                        {move || roots()
                            .into_iter()
                            .map(|category| view! {
                                <option value=category.id>{category.name}</option>
                            })
                            .collect_view()}
                    </select>
                    <textarea
                        class=format!("{INPUT} min-h-32 font-mono")
                        placeholder="Restaurants\nCaterers\nBakeries and Desserts"
                        prop:value=move || paste_body.get()
                        on:input=move |event| paste_body.set(event_target_value(&event))
                    />
                    <button type="button" on:click=paste prop:disabled=move || busy.get() class=PRIMARY_BTN>
                        "Add all"
                    </button>
                </div>
            </details>

            <div class="mt-4 space-y-2">{tree}</div>
        </section>
    }
}

/// One category, with its rename, reorder and delete controls.
#[component]
fn CategoryRow(
    category: ContactCategory,
    is_root: bool,
    busy: RwSignal<bool>,
    renaming: RwSignal<Option<String>>,
    rename_value: RwSignal<String>,
    on_move: Callback<(ContactCategory, i32)>,
    on_rename: Callback<String>,
    on_delete: Callback<String>,
) -> impl IntoView {
    let id = category.id.clone();
    let name = category.name.clone();
    let editing = {
        let id = id.clone();
        move || renaming.get().as_deref() == Some(id.as_str())
    };

    let begin = {
        let id = id.clone();
        let name = name.clone();
        move |_| {
            rename_value.set(name.clone());
            renaming.set(Some(id.clone()));
        }
    };
    let commit = {
        let id = id.clone();
        move |_| on_rename.run(id.clone())
    };
    let up = {
        let category = category.clone();
        move |_| on_move.run((category.clone(), -1))
    };
    let down = {
        let category = category.clone();
        move |_| on_move.run((category.clone(), 1))
    };
    let delete = {
        let id = id.clone();
        move |_| on_delete.run(id.clone())
    };

    view! {
        <div class="flex items-center gap-2 rounded-md px-2 py-1 hover:bg-slate-900">
            <Show
                when=editing.clone()
                fallback={
                    let name = name.clone();
                    move || view! {
                        <span class=if is_root { "flex-1 text-sm font-medium text-slate-200" } else { "flex-1 text-sm text-slate-300" }>
                            {name.clone()}
                        </span>
                    }
                }
            >
                <input
                    class=format!("{INPUT} flex-1")
                    prop:value=move || rename_value.get()
                    on:input=move |event| rename_value.set(event_target_value(&event))
                />
            </Show>
            <div class="flex shrink-0 gap-1">
                <Show
                    when=editing
                    fallback=move || view! {
                        <button type="button" on:click=begin.clone() prop:disabled=move || busy.get() class=BTN>
                            "Rename"
                        </button>
                    }
                >
                    <button type="button" on:click=commit.clone() prop:disabled=move || busy.get() class=BTN>
                        "Save"
                    </button>
                </Show>
                <button type="button" on:click=up.clone() prop:disabled=move || busy.get() class=BTN title="Move up">
                    "\u{2191}"
                </button>
                <button type="button" on:click=down.clone() prop:disabled=move || busy.get() class=BTN title="Move down">
                    "\u{2193}"
                </button>
                <button
                    type="button"
                    on:click=delete.clone()
                    prop:disabled=move || busy.get()
                    class="rounded-full border border-rose-500/40 px-3 py-1.5 text-xs font-medium text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                >
                    "Delete"
                </button>
            </div>
        </div>
    }
}

/// Create and edit the rules that drive suggested categories and fields.
#[component]
fn RulesManager(
    categories: RwSignal<Vec<ContactCategory>>,
    rules: RwSignal<Vec<ContactRule>>,
    busy: RwSignal<bool>,
    error: RwSignal<String>,
    notice: RwSignal<String>,
) -> impl IntoView {
    // `None` = not editing; `Some(None)` = a new rule; `Some(Some(id))` = editing.
    let editing = RwSignal::new(None::<Option<String>>);
    let draft = RwSignal::new(RuleInput::default());

    let begin_new = move |_| {
        draft.set(RuleInput {
            active: true,
            groups: vec![RuleGroupInput {
                min_choices: 0,
                ..Default::default()
            }],
            ..Default::default()
        });
        editing.set(Some(None));
    };

    let begin_edit = move |rule: ContactRule| {
        draft.set(RuleInput {
            name: rule.name.clone(),
            description: rule.description.clone(),
            trigger: Some(rule.trigger.clone()),
            active: rule.active,
            groups: rule
                .groups
                .iter()
                .map(|group| RuleGroupInput {
                    label: group.label.clone(),
                    help_text: group.help_text.clone(),
                    min_choices: group.min_choices,
                    max_choices: group.max_choices,
                    category_ids: group
                        .options
                        .iter()
                        .map(|option| option.category_id.clone())
                        .collect(),
                })
                .collect(),
            fields: rule.fields.clone(),
        });
        editing.set(Some(Some(rule.id.clone())));
    };

    let save = move |_| {
        let input = draft.get_untracked();
        let rule_id = editing.get_untracked().flatten();
        busy.set(true);
        error.set(String::new());
        notice.set(String::new());
        spawn_local(async move {
            match save_contact_rule(rule_id, input).await {
                Ok(items) => {
                    rules.try_set(items);
                    editing.try_set(None);
                    notice.try_set("Rule saved.".into());
                }
                Err(e) => {
                    error.try_set(err_text(e));
                }
            }
            busy.try_set(false);
        });
    };

    let remove = move |rule_id: String| {
        busy.set(true);
        error.set(String::new());
        spawn_local(async move {
            match delete_contact_rule(rule_id).await {
                Ok(items) => {
                    rules.try_set(items);
                }
                Err(e) => {
                    error.try_set(err_text(e));
                }
            }
            busy.try_set(false);
        });
    };

    let trigger_label = move |rule: &ContactRule| match &rule.trigger {
        RuleTrigger::ContactType(slug) => ContactType::from_slug(slug)
            .map(|value| format!("Contact type: {}", value.label()))
            .unwrap_or_else(|| format!("Contact type: {slug}")),
        RuleTrigger::Category(id) => categories
            .get()
            .into_iter()
            .find(|category| &category.id == id)
            .map(|category| format!("Category: {}", category.label()))
            .unwrap_or_else(|| "Category: (deleted)".to_string()),
    };

    view! {
        <section class=PANEL>
            <div class="flex flex-wrap items-start justify-between gap-3">
                <div>
                    <h2 class="text-sm font-semibold text-slate-200">"Rules"</h2>
                    <p class="mt-1 max-w-2xl text-xs text-slate-500">
                        "A rule offers grouped categories and blank property fields when a contact \
                         matches its trigger. A group capped at one choice shows as radio buttons."
                    </p>
                </div>
                <button type="button" on:click=begin_new prop:disabled=move || busy.get() class=PRIMARY_BTN>
                    "New rule"
                </button>
            </div>

            <Show when=move || editing.get().is_some()>
                <RuleEditor
                    draft
                    categories
                    busy
                    on_save=Callback::new(save)
                    on_cancel=Callback::new(move |()| editing.set(None))
                />
            </Show>

            <div class="mt-4 space-y-2">
                {move || {
                    let items = rules.get();
                    if items.is_empty() {
                        return view! {
                            <p class="text-xs text-slate-500">"No rules yet."</p>
                        }
                        .into_any();
                    }
                    items
                        .into_iter()
                        .map(|rule| {
                            let label = trigger_label(&rule);
                            let groups = rule.groups.len();
                            let fields = rule.fields.len();
                            let active = rule.active;
                            let id = rule.id.clone();
                            let for_edit = rule.clone();
                            view! {
                                <div class="flex flex-wrap items-center gap-3 rounded-lg border border-slate-800 bg-slate-950 px-3 py-2">
                                    <div class="min-w-48 flex-1">
                                        <p class="text-sm font-medium text-slate-200">
                                            {rule.name.clone()}
                                            {(!active).then(|| view! {
                                                <span class="ml-2 rounded-full bg-slate-700/40 px-2 py-0.5 text-[0.65rem] text-slate-300">
                                                    "Inactive"
                                                </span>
                                            })}
                                        </p>
                                        <p class="mt-0.5 text-xs text-slate-500">
                                            {label}
                                            {format!(" \u{00b7} {groups} group(s) \u{00b7} {fields} field(s)")}
                                        </p>
                                    </div>
                                    <div class="flex gap-1">
                                        <button
                                            type="button"
                                            on:click=move |_| begin_edit(for_edit.clone())
                                            prop:disabled=move || busy.get()
                                            class=BTN
                                        >
                                            "Edit"
                                        </button>
                                        <button
                                            type="button"
                                            on:click=move |_| remove(id.clone())
                                            prop:disabled=move || busy.get()
                                            class="rounded-full border border-rose-500/40 px-3 py-1.5 text-xs font-medium text-rose-300 hover:bg-rose-500/10 disabled:opacity-50"
                                        >
                                            "Delete"
                                        </button>
                                    </div>
                                </div>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </div>
        </section>
    }
}

/// The whole-rule form. The editor always submits every group and field, so a
/// removed group leaves nothing behind.
#[component]
fn RuleEditor(
    draft: RwSignal<RuleInput>,
    categories: RwSignal<Vec<ContactCategory>>,
    busy: RwSignal<bool>,
    on_save: Callback<()>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    // The trigger is one control over two vocabularies: "type:slug" or "cat:id".
    let trigger_value = move || match draft.get().trigger {
        Some(RuleTrigger::ContactType(slug)) => format!("type:{slug}"),
        Some(RuleTrigger::Category(id)) => format!("cat:{id}"),
        None => String::new(),
    };
    let set_trigger = move |raw: String| {
        let trigger = raw
            .split_once(':')
            .and_then(|(kind, value)| match kind {
                "type" => Some(RuleTrigger::ContactType(value.to_string())),
                "cat" => Some(RuleTrigger::Category(value.to_string())),
                _ => None,
            });
        draft.update(|draft| draft.trigger = trigger);
    };

    view! {
        <div class="mt-4 space-y-4 rounded-lg border border-primary-500/30 bg-primary-500/5 p-4">
            <div class="grid gap-3 sm:grid-cols-2">
                <label>
                    <span class=LABEL>"Rule name"</span>
                    <input
                        class=INPUT
                        prop:value=move || draft.get().name
                        on:input=move |event| draft.update(|d| d.name = event_target_value(&event))
                    />
                </label>
                <label>
                    <span class=LABEL>"Applies when"</span>
                    <select
                        class=INPUT
                        prop:value=trigger_value
                        on:change=move |event| set_trigger(event_target_value(&event))
                    >
                        <option value="">"Choose a trigger\u{2026}"</option>
                        <optgroup label="Contact type">
                            {ContactType::ALL.iter().map(|value| view! {
                                <option value=format!("type:{}", value.slug())>{value.label()}</option>
                            }).collect_view()}
                        </optgroup>
                        <optgroup label="Category">
                            {move || categories.get().into_iter().map(|category| view! {
                                <option value=format!("cat:{}", category.id)>{category.label()}</option>
                            }).collect_view()}
                        </optgroup>
                    </select>
                </label>
                <label class="sm:col-span-2">
                    <span class=LABEL>"Description (optional)"</span>
                    <input
                        class=INPUT
                        prop:value=move || draft.get().description
                        on:input=move |event| draft.update(|d| d.description = event_target_value(&event))
                    />
                </label>
            </div>

            <label class="flex items-center gap-2 text-sm text-slate-300">
                <input
                    type="checkbox"
                    class="h-4 w-4 accent-primary-500"
                    prop:checked=move || draft.get().active
                    on:change=move |event| {
                        let checked = event_target_checked(&event);
                        draft.update(|d| d.active = checked);
                    }
                />
                "Active"
            </label>

            <div>
                <div class="flex items-center justify-between">
                    <p class=LABEL>"Category groups"</p>
                    <button
                        type="button"
                        class=BTN
                        on:click=move |_| draft.update(|d| d.groups.push(RuleGroupInput::default()))
                    >
                        "Add group"
                    </button>
                </div>
                <div class="space-y-3">
                    {move || {
                        let count = draft.get().groups.len();
                        (0..count)
                            .map(|index| view! {
                                <RuleGroupEditor draft index categories />
                            })
                            .collect_view()
                    }}
                </div>
            </div>

            <div>
                <div class="flex items-center justify-between">
                    <p class=LABEL>"Property fields to add"</p>
                    <button
                        type="button"
                        class=BTN
                        on:click=move |_| draft.update(|d| d.fields.push(RuleField::default()))
                    >
                        "Add field"
                    </button>
                </div>
                <div class="space-y-2">
                    {move || {
                        let count = draft.get().fields.len();
                        (0..count)
                            .map(|index| view! {
                                <div class="rounded-lg border border-slate-800 bg-slate-950 p-2">
                                    <div class="grid gap-2 sm:grid-cols-[1fr_1fr_auto]">
                                        <input
                                            class=INPUT
                                            placeholder="Section (e.g. Vendor)"
                                            prop:value=move || draft.get().fields.get(index).map(|f| f.section.clone()).unwrap_or_default()
                                            on:input=move |event| {
                                                let value = event_target_value(&event);
                                                draft.update(|d| {
                                                    if let Some(field) = d.fields.get_mut(index) {
                                                        field.section = value;
                                                    }
                                                });
                                            }
                                        />
                                        <input
                                            class=INPUT
                                            placeholder="Field name (e.g. Lead time)"
                                            prop:value=move || draft.get().fields.get(index).map(|f| f.key.clone()).unwrap_or_default()
                                            on:input=move |event| {
                                                let value = event_target_value(&event);
                                                draft.update(|d| {
                                                    if let Some(field) = d.fields.get_mut(index) {
                                                        field.key = value;
                                                    }
                                                });
                                            }
                                        />
                                        <button
                                            type="button"
                                            class=BTN
                                            on:click=move |_| draft.update(|d| { d.fields.remove(index); })
                                        >
                                            "Remove"
                                        </button>
                                    </div>
                                    <label class="mt-2 flex items-center gap-2 px-1 text-xs text-slate-300">
                                        <input
                                            type="checkbox"
                                            class="h-4 w-4 accent-primary-500"
                                            prop:checked=move || draft.get().fields.get(index).is_some_and(|f| f.required)
                                            on:change=move |event| {
                                                let checked = event_target_checked(&event);
                                                draft.update(|d| {
                                                    if let Some(field) = d.fields.get_mut(index) {
                                                        field.required = checked;
                                                    }
                                                });
                                            }
                                        />
                                        "A value must be filled in when the contact is created"
                                    </label>
                                </div>
                            })
                            .collect_view()
                    }}
                </div>
            </div>

            <div class="flex gap-2">
                <button type="button" on:click=move |_| on_save.run(()) prop:disabled=move || busy.get() class=PRIMARY_BTN>
                    {move || if busy.get() { "Saving\u{2026}" } else { "Save rule" }}
                </button>
                <button type="button" on:click=move |_| on_cancel.run(()) class=BTN>
                    "Cancel"
                </button>
            </div>
        </div>
    }
}

/// One group within the rule form: its label, its limits, and its categories.
#[component]
fn RuleGroupEditor(
    draft: RwSignal<RuleInput>,
    index: usize,
    categories: RwSignal<Vec<ContactCategory>>,
) -> impl IntoView {
    let group = move || draft.get().groups.get(index).cloned().unwrap_or_default();

    // "Pick one" and "pick any" are the two shapes that matter; the rest is
    // expressible but rarely wanted, so the control offers these two directly.
    let single = move || group().max_choices == Some(1);
    let required = move || group().min_choices >= 1;

    view! {
        <div class="rounded-lg border border-slate-800 bg-slate-950 p-3">
            <div class="grid gap-2 sm:grid-cols-[1fr_auto]">
                <input
                    class=INPUT
                    placeholder="Group label (e.g. Location)"
                    prop:value=move || group().label
                    on:input=move |event| {
                        let value = event_target_value(&event);
                        draft.update(|d| {
                            if let Some(group) = d.groups.get_mut(index) {
                                group.label = value;
                            }
                        });
                    }
                />
                <button
                    type="button"
                    class=BTN
                    on:click=move |_| draft.update(|d| { d.groups.remove(index); })
                >
                    "Remove group"
                </button>
            </div>
            <div class="mt-2 flex flex-wrap gap-4">
                <label class="flex items-center gap-2 text-xs text-slate-300">
                    <input
                        type="checkbox"
                        class="h-4 w-4 accent-primary-500"
                        prop:checked=single
                        on:change=move |event| {
                            let checked = event_target_checked(&event);
                            draft.update(|d| {
                                if let Some(group) = d.groups.get_mut(index) {
                                    group.max_choices = checked.then_some(1);
                                }
                            });
                        }
                    />
                    "Only one may be chosen"
                </label>
                <label class="flex items-center gap-2 text-xs text-slate-300">
                    <input
                        type="checkbox"
                        class="h-4 w-4 accent-primary-500"
                        prop:checked=required
                        on:change=move |event| {
                            let checked = event_target_checked(&event);
                            draft.update(|d| {
                                if let Some(group) = d.groups.get_mut(index) {
                                    group.min_choices = i32::from(checked);
                                }
                            });
                        }
                    />
                    "Expected (shows a reminder when empty)"
                </label>
            </div>
            <div class="mt-2 max-h-48 overflow-y-auto rounded-lg border border-slate-800 p-2">
                {move || categories
                    .get()
                    .into_iter()
                    .filter(|category| category.parent_id.is_some())
                    .map(|category| {
                        let id = category.id.clone();
                        let checked_id = id.clone();
                        view! {
                            <label class="flex cursor-pointer items-start gap-2 rounded-md px-2 py-1 text-xs text-slate-300 hover:bg-slate-900">
                                <input
                                    type="checkbox"
                                    class="mt-0.5 accent-primary-500"
                                    prop:checked=move || {
                                        draft.get().groups.get(index)
                                            .is_some_and(|group| group.category_ids.contains(&checked_id))
                                    }
                                    on:change=move |event| {
                                        let checked = event_target_checked(&event);
                                        let id = id.clone();
                                        draft.update(|d| {
                                            if let Some(group) = d.groups.get_mut(index) {
                                                if checked {
                                                    if !group.category_ids.contains(&id) {
                                                        group.category_ids.push(id);
                                                    }
                                                } else {
                                                    group.category_ids.retain(|item| item != &id);
                                                }
                                            }
                                        });
                                    }
                                />
                                <span>{category.label()}</span>
                            </label>
                        }
                    })
                    .collect_view()}
            </div>
        </div>
    }
}
