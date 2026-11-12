//! The approved case plan: staff-agreed actions and the tasks assigned for them.
//!
//! Staff only. Reading needs `ViewCase`; adding an action needs `EditCase`; a
//! task's attempts and outcome can be recorded by its assignee or by anyone who
//! can edit.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use crate::server_fns::crm::coded_enum;

coded_enum!(TaskOutcome {
    Completed => ("completed", "Completed"),
    DeclinedByClient => ("declined_by_client", "Declined by client"),
    NoLongerNeeded => ("no_longer_needed", "No longer needed"),
});

coded_enum!(AttemptResult {
    CouldNotReach => ("could_not_reach", "Couldn't reach client"),
    LeftMessage => ("left_message", "Left a message"),
    TalkedNotDone => ("talked_not_done", "Talked, not done yet"),
    WaitingOnOthers => ("waiting_on_others", "Waiting on someone else"),
});

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskAttempt {
    pub result: AttemptResult,
    pub note: String,
    pub recorded_on: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CaseTask {
    pub id: String,
    pub assignee_id: String,
    pub assignee_name: String,
    pub follow_up_on: String,
    pub outcome: Option<TaskOutcome>,
    pub outcome_notes: String,
    pub outcome_recorded_on: String,
    pub last_attempt: Option<TaskAttempt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CasePlanAction {
    pub id: String,
    pub title: String,
    pub added_by_name: String,
    pub added_on: String,
    pub tasks: Vec<CaseTask>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlanAssignee {
    pub id: String,
    pub name: String,
}

#[server(prefix = "/api")]
pub async fn list_case_plan(case_id: String) -> Result<Vec<CasePlanAction>, ServerFnError> {
    use crate::server::db::case_plan;
    use crate::server::permissions::{require_cap, require_user};
    use crate::server_fns::capabilities::CaseCapability;

    let user = require_user().await?;
    crate::server_fns::crm::require_staff(&user)?;
    require_cap(&user, &case_id, CaseCapability::ViewCase).await?;
    case_plan::list(&case_id).await.map_err(ServerFnError::new)
}

#[server(prefix = "/api")]
pub async fn list_plan_assignees(case_id: String) -> Result<Vec<PlanAssignee>, ServerFnError> {
    use crate::server::db::case_plan;
    use crate::server::permissions::{require_cap, require_user};
    use crate::server_fns::capabilities::CaseCapability;

    let user = require_user().await?;
    crate::server_fns::crm::require_staff(&user)?;
    require_cap(&user, &case_id, CaseCapability::EditCase).await?;
    case_plan::assignees(&case_id)
        .await
        .map_err(ServerFnError::new)
}

#[server(prefix = "/api")]
pub async fn add_plan_action(
    case_id: String,
    title: String,
    assignee_id: String,
    follow_up_on: String,
) -> Result<(), ServerFnError> {
    use crate::server::db::case_plan;
    use crate::server::permissions::{require_cap, require_user};
    use crate::server_fns::capabilities::CaseCapability;
    use crate::server_fns::crm::{clean_date, clean_text, MAX_SHORT_TEXT};

    let user = require_user().await?;
    crate::server_fns::crm::require_staff(&user)?;
    require_cap(&user, &case_id, CaseCapability::EditCase).await?;
    let title = clean_text(&title, "action", MAX_SHORT_TEXT).map_err(ServerFnError::new)?;
    if title.is_empty() {
        return Err(ServerFnError::new("Describe the action."));
    }
    let follow_up_on = clean_date(&follow_up_on, "follow-up date").map_err(ServerFnError::new)?;
    reject_past(&follow_up_on, "The follow-up date")?;
    let assignee = case_plan::assignees(&case_id)
        .await
        .map_err(ServerFnError::new)?
        .into_iter()
        .find(|a| a.id == assignee_id)
        .ok_or_else(|| ServerFnError::new("Choose someone who is on this case."))?;
    case_plan::add_action(
        &case_id,
        &title,
        &assignee,
        &follow_up_on,
        &user.id,
        &user.full_name(),
    )
    .await
    .map_err(ServerFnError::new)
}

#[server(prefix = "/api")]
pub async fn log_task_attempt(
    task_id: String,
    result: AttemptResult,
    note: String,
    next_try: String,
) -> Result<(), ServerFnError> {
    use crate::server::db::case_plan;
    use crate::server::permissions::require_user;
    use crate::server_fns::crm::{clean_date, clean_text, MAX_SHORT_TEXT};

    let user = require_user().await?;
    let case_id = authorize_task_update(&user, &task_id).await?;
    let note = clean_text(&note, "note", MAX_SHORT_TEXT).map_err(ServerFnError::new)?;
    let next_try = clean_date(&next_try, "next try date").map_err(ServerFnError::new)?;
    if next_try.is_empty() {
        return Err(ServerFnError::new("Pick the date of the next try."));
    }
    reject_past(&next_try, "The next try")?;
    case_plan::log_attempt(
        &task_id,
        &case_id,
        result,
        &note,
        &next_try,
        &user.id,
        &user.full_name(),
    )
    .await
    .map_err(plan_error)
}

#[server(prefix = "/api")]
pub async fn record_task_outcome(
    task_id: String,
    outcome: TaskOutcome,
    notes: String,
) -> Result<(), ServerFnError> {
    use crate::server::db::case_plan;
    use crate::server::permissions::require_user;
    use crate::server_fns::crm::{clean_text, MAX_LONG_TEXT};

    let user = require_user().await?;
    let case_id = authorize_task_update(&user, &task_id).await?;
    let notes = clean_text(&notes, "outcome notes", MAX_LONG_TEXT).map_err(ServerFnError::new)?;
    case_plan::record_outcome(
        &task_id,
        &case_id,
        outcome,
        &notes,
        &user.id,
        &user.full_name(),
    )
    .await
    .map_err(plan_error)
}

/// The assignee needs `AddNotes` or `EditCase`, anyone else `EditCase`; returns the stored
/// task's case.
#[cfg(feature = "ssr")]
async fn authorize_task_update(
    user: &crate::server_fns::users::User,
    task_id: &str,
) -> Result<String, ServerFnError> {
    use crate::server::db::case_plan;
    use crate::server::permissions::require_cap;
    use crate::server_fns::capabilities::CaseCapability;

    crate::server_fns::crm::require_staff(user)?;
    let owner = case_plan::task_owner(task_id)
        .await
        .map_err(ServerFnError::new)?
        .ok_or_else(|| ServerFnError::new("Task not found."))?;
    if owner.closed {
        return Err(ServerFnError::new("This task already has a final outcome."));
    }
    let is_assignee = owner.assignee_id.as_deref() == Some(user.id.as_str());
    if is_assignee
        && require_cap(user, &owner.case_id, CaseCapability::AddNotes)
            .await
            .is_ok()
    {
        return Ok(owner.case_id);
    }
    require_cap(user, &owner.case_id, CaseCapability::EditCase).await?;
    Ok(owner.case_id)
}

/// A date the plan schedules work for must not be in the past. A day of slack,
/// because the server's clock may run ahead of the user's time zone.
#[cfg(feature = "ssr")]
fn reject_past(date: &str, label: &str) -> Result<(), ServerFnError> {
    let earliest = (chrono::Local::now() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    if !date.is_empty() && date < earliest.as_str() {
        return Err(ServerFnError::new(format!("{label} can't be in the past.")));
    }
    Ok(())
}

/// The database layer reports a lost race on a task as a protocol error whose text is meant
/// for the user.
#[cfg(feature = "ssr")]
fn plan_error(error: sqlx::Error) -> ServerFnError {
    match error {
        sqlx::Error::Protocol(message) => ServerFnError::new(message),
        other => ServerFnError::new(other),
    }
}
