//! Approved case plan persistence. Audit entries are volunteer-only, like the
//! plan itself.

use crate::helpers::visibility::Visibility;
use crate::server::db::{audit, ids, now_stamp, pool};
use crate::server_fns::case_plan::{
    AttemptResult, CasePlanAction, CaseTask, PlanAssignee, TaskAttempt, TaskOutcome,
};

#[derive(sqlx::FromRow)]
struct PlanRow {
    action_id: String,
    title: String,
    added_by_name: String,
    added_on: String,
    task_id: Option<String>,
    assignee_id: Option<String>,
    assignee_name: Option<String>,
    follow_up_on: Option<String>,
    outcome: Option<String>,
    outcome_notes: Option<String>,
    outcome_recorded_on: Option<String>,
    attempt_result: Option<String>,
    attempt_note: Option<String>,
    attempt_on: Option<String>,
}

pub struct TaskOwner {
    pub case_id: String,
    pub assignee_id: Option<String>,
    pub closed: bool,
}

pub async fn list(case_id: &str) -> Result<Vec<CasePlanAction>, sqlx::Error> {
    let rows = sqlx::query_as::<_, PlanRow>(
        "SELECT a.id AS action_id, a.title, a.added_by_name,
                to_char(a.added_on, 'YYYY-MM-DD') AS added_on,
                t.id AS task_id, t.assignee_id, t.assignee_name,
                to_char(t.follow_up_on, 'YYYY-MM-DD') AS follow_up_on,
                t.outcome, t.outcome_notes,
                to_char(t.outcome_recorded_on, 'YYYY-MM-DD') AS outcome_recorded_on,
                at.result AS attempt_result, at.note AS attempt_note,
                to_char(at.recorded_on, 'YYYY-MM-DD') AS attempt_on
         FROM case_plan_actions a
         LEFT JOIN case_tasks t ON t.action_id = a.id
         LEFT JOIN LATERAL (
             SELECT result, note, recorded_on FROM case_task_attempts
             WHERE task_id = t.id ORDER BY seq DESC LIMIT 1
         ) at ON true
         WHERE a.case_id = $1
         ORDER BY a.seq, t.seq",
    )
    .bind(case_id)
    .fetch_all(pool())
    .await?;

    let mut actions: Vec<CasePlanAction> = Vec::new();
    for row in rows {
        if actions
            .last()
            .map(|a| a.id != row.action_id)
            .unwrap_or(true)
        {
            actions.push(CasePlanAction {
                id: row.action_id,
                title: row.title,
                added_by_name: row.added_by_name,
                added_on: row.added_on,
                tasks: Vec::new(),
            });
        }
        if let (Some(id), Some(action)) = (row.task_id, actions.last_mut()) {
            action.tasks.push(CaseTask {
                id,
                assignee_id: row.assignee_id.unwrap_or_default(),
                assignee_name: row.assignee_name.unwrap_or_default(),
                follow_up_on: row.follow_up_on.unwrap_or_default(),
                outcome: row.outcome.as_deref().and_then(TaskOutcome::from_slug),
                outcome_notes: row.outcome_notes.unwrap_or_default(),
                outcome_recorded_on: row.outcome_recorded_on.unwrap_or_default(),
                last_attempt: row
                    .attempt_result
                    .as_deref()
                    .and_then(AttemptResult::from_slug)
                    .map(|result| TaskAttempt {
                        result,
                        note: row.attempt_note.unwrap_or_default(),
                        recorded_on: row.attempt_on.unwrap_or_default(),
                    }),
            });
        }
    }
    Ok(actions)
}

pub async fn assignees(case_id: &str) -> Result<Vec<PlanAssignee>, sqlx::Error> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT u.id, btrim(u.first_name || ' ' || u.last_name)
         FROM users u
         JOIN case_assignments ca ON ca.user_id = u.id
         WHERE ca.case_id = $1 AND ca.capability = 'view_case'
           AND EXISTS (
               SELECT 1 FROM case_assignments n
               WHERE n.case_id = ca.case_id AND n.user_id = ca.user_id
                 AND n.capability = 'add_notes'
           )
           AND u.role IN ('volunteer', 'operations_admin', 'site_admin')
         ORDER BY u.first_name, u.last_name",
    )
    .bind(case_id)
    .fetch_all(pool())
    .await
    .map(|rows| {
        rows.into_iter()
            .map(|(id, name)| PlanAssignee { id, name })
            .collect()
    })
}

pub async fn add_action(
    case_id: &str,
    title: &str,
    assignee: &PlanAssignee,
    follow_up_on: &str,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let action_id = ids::next(&mut *tx, "pa").await?;
    sqlx::query(
        "INSERT INTO case_plan_actions (id, case_id, title, added_by_id, added_by_name, added_on)
         VALUES ($1, $2, $3, $4, $5, $6::date)",
    )
    .bind(&action_id)
    .bind(case_id)
    .bind(title)
    .bind(actor_user_id)
    .bind(actor)
    .bind(today())
    .execute(&mut *tx)
    .await?;
    let task_id = ids::next(&mut *tx, "pt").await?;
    sqlx::query(
        "INSERT INTO case_tasks (id, action_id, assignee_id, assignee_name, follow_up_on)
         VALUES ($1, $2, $3, $4, NULLIF($5, '')::date)",
    )
    .bind(&task_id)
    .bind(&action_id)
    .bind(&assignee.id)
    .bind(&assignee.name)
    .bind(follow_up_on)
    .execute(&mut *tx)
    .await?;
    audit::record_in_transaction_with_visibility(
        &mut tx,
        audit::Entity::Case,
        case_id,
        actor,
        "case plan",
        "",
        &format!("added \"{title}\", assigned to {}", assignee.name),
        Visibility::VolunteerOnly,
    )
    .await?;
    tx.commit().await
}

pub async fn task_owner(task_id: &str) -> Result<Option<TaskOwner>, sqlx::Error> {
    sqlx::query_as::<_, (String, Option<String>, bool)>(
        "SELECT a.case_id, t.assignee_id, t.outcome <> ''
         FROM case_tasks t JOIN case_plan_actions a ON a.id = t.action_id
         WHERE t.id = $1",
    )
    .bind(task_id)
    .fetch_optional(pool())
    .await
    .map(|row| {
        row.map(|(case_id, assignee_id, closed)| TaskOwner {
            case_id,
            assignee_id,
            closed,
        })
    })
}

pub async fn log_attempt(
    task_id: &str,
    case_id: &str,
    result: AttemptResult,
    note: &str,
    next_try: &str,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let title = lock_open_task(&mut tx, task_id).await?;
    let attempt_id = ids::next(&mut *tx, "pq").await?;
    sqlx::query(
        "INSERT INTO case_task_attempts
             (id, task_id, result, note, next_try, recorded_by_id, recorded_by_name, recorded_on)
         VALUES ($1, $2, $3, $4, $5::date, $6, $7, $8::date)",
    )
    .bind(&attempt_id)
    .bind(task_id)
    .bind(result.slug())
    .bind(note)
    .bind(next_try)
    .bind(actor_user_id)
    .bind(actor)
    .bind(today())
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE case_tasks SET follow_up_on = $2::date WHERE id = $1")
        .bind(task_id)
        .bind(next_try)
        .execute(&mut *tx)
        .await?;
    audit::record_in_transaction_with_visibility(
        &mut tx,
        audit::Entity::Case,
        case_id,
        actor,
        &format!("task: {title}"),
        "",
        &format!("attempt: {}, next try {next_try}", result.label()),
        Visibility::VolunteerOnly,
    )
    .await?;
    tx.commit().await
}

pub async fn record_outcome(
    task_id: &str,
    case_id: &str,
    outcome: TaskOutcome,
    notes: &str,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let title = lock_open_task(&mut tx, task_id).await?;
    sqlx::query(
        "UPDATE case_tasks SET
             outcome = $2,
             outcome_notes = $3,
             outcome_recorded_by = $4,
             outcome_recorded_on = $5::date
         WHERE id = $1",
    )
    .bind(task_id)
    .bind(outcome.slug())
    .bind(notes)
    .bind(actor)
    .bind(today())
    .execute(&mut *tx)
    .await?;
    audit::record_in_transaction_with_visibility(
        &mut tx,
        audit::Entity::Case,
        case_id,
        actor,
        &format!("task: {title}"),
        "Active",
        outcome.label(),
        Visibility::VolunteerOnly,
    )
    .await?;
    tx.commit().await
}

async fn lock_open_task(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    task_id: &str,
) -> Result<String, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT a.title
         FROM case_tasks t JOIN case_plan_actions a ON a.id = t.action_id
         WHERE t.id = $1 AND t.outcome = '' FOR UPDATE OF t",
    )
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| sqlx::Error::Protocol("This task already has a final outcome.".into()))
}

fn today() -> String {
    now_stamp()[..10].to_string()
}
