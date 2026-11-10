//! Persistence for contact-mail recipient selection and start/final history writes.

use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use std::collections::HashSet;

use crate::server::db::{ids, pool, property_filters};
use crate::server_fns::contact_mail::{
    ContactMailCandidate, ContactMailFailure, ContactMailFilters, ContactMailSelection,
    ContactMailTask, ContactMailTaskStatus,
};
use crate::server_fns::contacts::ContactType;
use crate::server_fns::pagination::Page;
use crate::server_fns::property_filters::PropertySubject;

const STAMP: &str = "%Y-%m-%d %H:%M:%S";
const ELIGIBLE_FROM: &str = "FROM contacts c
    LEFT JOIN organizations o ON o.id = c.organization_id
    LEFT JOIN users u ON u.id = c.user_id";
const EFFECTIVE_EMAIL: &str = "CASE WHEN u.id IS NULL THEN c.email ELSE u.email END";
const EFFECTIVE_NAME: &str = "coalesce(
    nullif(btrim(coalesce(nullif(c.preferred_name, ''),
        CASE WHEN u.id IS NULL THEN c.first_name ELSE u.first_name END)
        || ' ' || CASE WHEN u.id IS NULL THEN c.last_name ELSE u.last_name END), ''),
    nullif(o.name, ''), c.id)";

#[derive(sqlx::FromRow)]
struct CandidateRow {
    id: String,
    name: String,
    email: String,
    organization: String,
    types: Vec<String>,
}

impl From<CandidateRow> for ContactMailCandidate {
    fn from(row: CandidateRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            email: row.email,
            organization: row.organization,
            types: row
                .types
                .iter()
                .filter_map(|slug| ContactType::from_slug(slug))
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MailRecipientRecord {
    pub position: i32,
    pub contact_id: String,
    pub name: String,
    pub email: String,
    pub status: String,
    pub attempt_started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: String,
}

#[derive(Clone, Debug)]
pub struct MailTaskRecord {
    pub id: String,
    pub status: ContactMailTaskStatus,
    pub subject: String,
    pub body: String,
    pub created_by_name: String,
    pub recipient_total: i32,
    pub accepted_count: i32,
    pub failed_count: i32,
    pub created_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub next_send_at: Option<DateTime<Utc>>,
    pub cancel_requested_at: Option<DateTime<Utc>>,
    pub cancel_requested_by_id: Option<String>,
    pub cancel_requested_by_name: String,
    pub error: String,
    pub recipients: Vec<MailRecipientRecord>,
}

impl MailTaskRecord {
    pub fn to_public(&self) -> ContactMailTask {
        ContactMailTask {
            id: self.id.clone(),
            status: self.status,
            subject: self.subject.clone(),
            body: self.body.clone(),
            created_by_name: self.created_by_name.clone(),
            recipient_total: self.recipient_total,
            accepted_count: self.accepted_count,
            failed_count: self.failed_count,
            created_at: stamp(Some(self.created_at)),
            started_at: stamp(Some(self.started_at)),
            completed_at: stamp(self.completed_at),
            next_send_at: stamp(self.next_send_at),
            cancel_requested_at: stamp(self.cancel_requested_at),
            cancel_requested_by_name: self.cancel_requested_by_name.clone(),
            error: self.error.clone(),
            failures: self
                .recipients
                .iter()
                .filter(|recipient| recipient.status == "failed")
                .map(|recipient| ContactMailFailure {
                    name: recipient.name.clone(),
                    email: recipient.email.clone(),
                    error: recipient.error.clone(),
                })
                .collect(),
        }
    }
}

/// The eligible-recipient CTE. The SQL is the same whatever is filtered — the
/// property chips arrive as one `jsonb` bind — so both the counting and the
/// sending path share it and cannot disagree about who matches.
fn candidate_cte() -> String {
    // A typed word also matches a property value, and the property chips narrow
    // the same set.
    let property_search_sql =
        property_filters::keyword_match_sql(PropertySubject::Contact, "c.id", 2);
    let property_filter_sql = property_filters::predicate_sql(PropertySubject::Contact, "c.id", 6);
    format!(
        "WITH eligible AS (
            SELECT c.id, {EFFECTIVE_NAME} AS name, btrim({EFFECTIVE_EMAIL}) AS email,
                   coalesce(o.name, '') AS organization, c.types,
                   row_number() OVER (
                       PARTITION BY lower(btrim({EFFECTIVE_EMAIL}))
                       ORDER BY lower(c.last_name), lower(c.first_name), c.id
                   ) AS email_rank
            {ELIGIBLE_FROM}
            WHERE NOT c.archived AND NOT c.do_not_contact
              AND btrim({EFFECTIVE_EMAIL}) <> '' AND {EFFECTIVE_EMAIL} LIKE '%@%'
              AND ($1 = '' OR c.first_name ILIKE $2 ESCAPE '\\'
                   OR c.last_name ILIKE $2 ESCAPE '\\'
                   OR c.preferred_name ILIKE $2 ESCAPE '\\'
                   OR {EFFECTIVE_NAME} ILIKE $2 ESCAPE '\\'
                   OR {EFFECTIVE_EMAIL} ILIKE $2 ESCAPE '\\'
                   OR o.name ILIKE $2 ESCAPE '\\'
                   {property_search_sql})
              AND (cardinality($3::text[]) = 0 OR c.id IN (
                    SELECT a.contact_id FROM contact_category_assignments a
                    WHERE a.category_id = ANY($3::text[])
                    GROUP BY a.contact_id
                    HAVING count(DISTINCT a.category_id) = cardinality($3::text[])
              ))
              AND ($4 = '' OR $4 = ANY(c.types))
              AND ($5 = '' OR c.organization_id = $5)
              {property_filter_sql}
        )"
    )
}

fn search_pattern(query: &str) -> String {
    format!(
        "%{}%",
        query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

fn type_slug(filters: &ContactMailFilters) -> &'static str {
    filters
        .contact_type
        .map(ContactType::slug)
        .unwrap_or_default()
}

pub async fn candidate_page(
    filters: &ContactMailFilters,
    offset: i64,
    limit: i64,
) -> Result<Page<ContactMailCandidate>, sqlx::Error> {
    let cte = candidate_cte();
    let pattern = search_pattern(&filters.query);
    let filters_json = property_filters::to_json(&filters.property_filters);
    let total: i64 = sqlx::query_scalar(&format!(
        "{cte} SELECT count(*) FROM eligible WHERE email_rank = 1"
    ))
    .bind(&filters.query)
    .bind(&pattern)
    .bind(&filters.category_ids)
    .bind(type_slug(filters))
    .bind(&filters.organization_id)
    .bind(&filters_json)
    .fetch_one(pool())
    .await?;
    let rows = sqlx::query_as::<_, CandidateRow>(&format!(
        "{cte}
         SELECT id, name, email, organization, types FROM eligible
         WHERE email_rank = 1
         ORDER BY lower(name), id LIMIT $7 OFFSET $8"
    ))
    .bind(&filters.query)
    .bind(&pattern)
    .bind(&filters.category_ids)
    .bind(type_slug(filters))
    .bind(&filters.organization_id)
    .bind(&filters_json)
    .bind(limit.clamp(1, 100))
    .bind(offset.max(0))
    .fetch_all(pool())
    .await?;
    Ok(Page {
        items: rows.into_iter().map(Into::into).collect(),
        total,
    })
}

async fn selected_recipients(
    tx: &mut Transaction<'_, Postgres>,
    selection: &ContactMailSelection,
) -> Result<Vec<CandidateRow>, sqlx::Error> {
    let unfiltered = ContactMailFilters::default();
    let filters = if selection.all_matching {
        &selection.filters
    } else {
        &unfiltered
    };
    let pattern = search_pattern(&filters.query);
    let filters_json = property_filters::to_json(&filters.property_filters);
    let cte = candidate_cte();
    sqlx::query_as::<_, CandidateRow>(&format!(
        "{cte}
         SELECT id, name, email, organization, types FROM eligible
         WHERE email_rank = 1
           AND (($7 AND NOT (id = ANY($8::text[])))
                OR (NOT $7 AND id = ANY($9::text[])))
         ORDER BY lower(name), id"
    ))
    .bind(&filters.query)
    .bind(&pattern)
    .bind(&filters.category_ids)
    .bind(type_slug(filters))
    .bind(&filters.organization_id)
    .bind(&filters_json)
    .bind(selection.all_matching)
    .bind(&selection.excluded_contact_ids)
    .bind(&selection.contact_ids)
    .fetch_all(&mut **tx)
    .await
}

/// Resolve eligible recipients and commit the immutable start record once.
pub async fn start_task(
    selection: &ContactMailSelection,
    subject: &str,
    body: &str,
    creator_id: &str,
    creator_name: &str,
) -> Result<MailTaskRecord, sqlx::Error> {
    let mut tx = pool().begin().await?;
    let recipients = selected_recipients(&mut tx, selection).await?;
    if recipients.is_empty() {
        return Err(sqlx::Error::Protocol(
            "Choose at least one eligible contact with an email address.".into(),
        ));
    }
    if !selection.all_matching && recipients.len() != selection.contact_ids.len() {
        return Err(sqlx::Error::Protocol(
            "One or more selected contacts are no longer eligible or share an email address. Review the selection and try again."
                .into(),
        ));
    }

    let id = ids::opaque("mail");
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO contact_mail_tasks
            (id, status, subject, body, created_by_id, created_by_name,
             recipient_total, started_at, next_send_at)
         VALUES ($1, 'running', $2, $3, $4, $5, $6, $7, $7)",
    )
    .bind(&id)
    .bind(subject)
    .bind(body)
    .bind(creator_id)
    .bind(creator_name)
    .bind(recipients.len() as i32)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    let mut recipient_records = Vec::with_capacity(recipients.len());
    for (index, recipient) in recipients.into_iter().enumerate() {
        let position = index as i32 + 1;
        sqlx::query(
            "INSERT INTO contact_mail_recipients
                (task_id, position, contact_id, name, email)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id)
        .bind(position)
        .bind(&recipient.id)
        .bind(&recipient.name)
        .bind(&recipient.email)
        .execute(&mut *tx)
        .await?;
        recipient_records.push(MailRecipientRecord {
            position,
            contact_id: recipient.id,
            name: recipient.name,
            email: recipient.email,
            status: "pending".to_string(),
            attempt_started_at: None,
            finished_at: None,
            error: String::new(),
        });
    }
    tx.commit().await?;

    Ok(MailTaskRecord {
        id,
        status: ContactMailTaskStatus::Running,
        subject: subject.to_string(),
        body: body.to_string(),
        created_by_name: creator_name.to_string(),
        recipient_total: recipient_records.len() as i32,
        accepted_count: 0,
        failed_count: 0,
        created_at: now,
        started_at: now,
        completed_at: None,
        next_send_at: Some(now),
        cancel_requested_at: None,
        cancel_requested_by_id: None,
        cancel_requested_by_name: String::new(),
        error: String::new(),
        recipients: recipient_records,
    })
}

#[derive(sqlx::FromRow)]
struct BlockedRow {
    name: String,
    email: String,
    do_not_contact: bool,
}

/// Explicitly selected contacts that would now be skipped, so the UI can warn
/// before sending. Only meaningful for explicit id selections: "all matching"
/// is re-resolved through the eligibility CTE, which already excludes these.
pub async fn blocked_selection_preview(
    contact_ids: &[String],
) -> Result<Vec<(String, String, bool)>, sqlx::Error> {
    if contact_ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, BlockedRow>(&format!(
        "SELECT {EFFECTIVE_NAME} AS name, btrim({EFFECTIVE_EMAIL}) AS email, c.do_not_contact
         {ELIGIBLE_FROM}
         WHERE c.id = ANY($1::text[]) AND (c.do_not_contact OR c.archived)
         ORDER BY lower({EFFECTIVE_NAME}), c.id"
    ))
    .bind(contact_ids)
    .fetch_all(pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.name, row.email, row.do_not_contact))
        .collect())
}

/// Contacts that must not be mailed right now: flagged "do not contact" or
/// archived since the task started. Used as a pre-send fail-safe.
pub async fn suppressed_contact_ids(
    contact_ids: &[String],
) -> Result<HashSet<String>, sqlx::Error> {
    if contact_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM contacts WHERE id = ANY($1::text[]) AND (do_not_contact OR archived)",
    )
    .bind(contact_ids)
    .fetch_all(pool())
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Addresses belonging to any contact flagged "do not contact", matched
/// case-insensitively on the effective (contact or linked user) email.
pub async fn do_not_contact_addresses(
    addresses: &[String],
) -> Result<HashSet<String>, sqlx::Error> {
    if addresses.is_empty() {
        return Ok(HashSet::new());
    }
    let lowered: Vec<String> = addresses
        .iter()
        .map(|address| address.trim().to_lowercase())
        .collect();
    let rows: Vec<(String,)> = sqlx::query_as(&format!(
        "SELECT DISTINCT lower(btrim({EFFECTIVE_EMAIL})) AS email
         {ELIGIBLE_FROM}
         WHERE c.do_not_contact AND lower(btrim({EFFECTIVE_EMAIL})) = ANY($1::text[])"
    ))
    .bind(&lowered)
    .fetch_all(pool())
    .await?;
    Ok(rows.into_iter().map(|(email,)| email).collect())
}

/// Commit one terminal task snapshot and all final recipient outcomes once.
pub async fn finish_task(task: &MailTaskRecord) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    sqlx::query(
        "UPDATE contact_mail_tasks
         SET status = $2, accepted_count = $3, failed_count = $4,
             completed_at = $5, next_send_at = NULL, cancel_requested_at = $6,
             cancel_requested_by_id = $7, cancel_requested_by_name = $8, error = $9
         WHERE id = $1",
    )
    .bind(&task.id)
    .bind(status_slug(task.status))
    .bind(task.accepted_count)
    .bind(task.failed_count)
    .bind(task.completed_at)
    .bind(task.cancel_requested_at)
    .bind(&task.cancel_requested_by_id)
    .bind(&task.cancel_requested_by_name)
    .bind(&task.error)
    .execute(&mut *tx)
    .await?;

    for recipient in &task.recipients {
        sqlx::query(
            "UPDATE contact_mail_recipients
             SET status = $3, attempt_started_at = $4, finished_at = $5, error = $6
             WHERE task_id = $1 AND position = $2",
        )
        .bind(&task.id)
        .bind(recipient.position)
        .bind(&recipient.status)
        .bind(recipient.attempt_started_at)
        .bind(recipient.finished_at)
        .bind(&recipient.error)
        .execute(&mut *tx)
        .await?;
    }

    // Log the send on every contact it reached, in the same transaction as the
    // outcomes, so a contact's outreach history matches what was delivered.
    // The unique index on (mail_task_id, contact_id) keeps a retry from
    // logging twice.
    sqlx::query(
        "INSERT INTO contact_communications
             (id, contact_id, kind, body, author_id, author_name, occurred_at, mail_task_id)
         SELECT 'comm-' || replace(gen_random_uuid()::text, '-', ''),
                r.contact_id, 'outreach', 'Group email: ' || t.subject,
                t.created_by_id, t.created_by_name,
                COALESCE(r.finished_at, now()), t.id
         FROM contact_mail_recipients r
         JOIN contact_mail_tasks t ON t.id = r.task_id
         JOIN contacts c ON c.id = r.contact_id
         WHERE r.task_id = $1 AND r.status = 'accepted'
         ON CONFLICT DO NOTHING",
    )
    .bind(&task.id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await
}

/// Close tasks interrupted by a process stop; their in-memory outcomes are unknown.
pub async fn fail_interrupted_tasks() -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    let detail =
        "The server stopped before this mail task finished; final recipient outcomes are unknown.";
    sqlx::query(
        "UPDATE contact_mail_recipients recipient
         SET status = 'failed', finished_at = now(), error = $1
         FROM contact_mail_tasks task
         WHERE recipient.task_id = task.id
           AND task.status IN ('queued', 'running', 'cancelling')
           AND recipient.status IN ('pending', 'sending')",
    )
    .bind(detail)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE contact_mail_tasks task
         SET status = 'failed', completed_at = now(), next_send_at = NULL,
             accepted_count = outcomes.accepted_count,
             failed_count = outcomes.failed_count, error = $1
         FROM (
             SELECT task_id,
                    count(*) FILTER (WHERE status = 'accepted')::integer AS accepted_count,
                    count(*) FILTER (WHERE status = 'failed')::integer AS failed_count
             FROM contact_mail_recipients GROUP BY task_id
         ) outcomes
         WHERE task.id = outcomes.task_id
           AND task.status IN ('queued', 'running', 'cancelling')",
    )
    .bind(detail)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

#[derive(sqlx::FromRow)]
struct TaskRow {
    id: String,
    status: String,
    subject: String,
    body: String,
    created_by_name: String,
    recipient_total: i32,
    accepted_count: i32,
    failed_count: i32,
    created_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    next_send_at: Option<DateTime<Utc>>,
    cancel_requested_at: Option<DateTime<Utc>>,
    cancel_requested_by_name: String,
    error: String,
}

pub async fn latest_task() -> Result<Option<ContactMailTask>, sqlx::Error> {
    let row = sqlx::query_as::<_, TaskRow>(
        "SELECT id, status, subject, body, created_by_name, recipient_total,
                accepted_count, failed_count, created_at, started_at, completed_at,
                next_send_at, cancel_requested_at, cancel_requested_by_name, error
         FROM contact_mail_tasks ORDER BY seq DESC LIMIT 1",
    )
    .fetch_optional(pool())
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let status = ContactMailTaskStatus::from_slug(&row.status)
        .ok_or_else(|| sqlx::Error::Protocol("Unknown contact mail task status.".into()))?;
    let failures = sqlx::query_as::<_, (String, String, String)>(
        "SELECT name, email, error FROM contact_mail_recipients
         WHERE task_id = $1 AND status = 'failed' ORDER BY position",
    )
    .bind(&row.id)
    .fetch_all(pool())
    .await?
    .into_iter()
    .map(|(name, email, error)| ContactMailFailure { name, email, error })
    .collect();
    Ok(Some(ContactMailTask {
        id: row.id,
        status,
        subject: row.subject,
        body: row.body,
        created_by_name: row.created_by_name,
        recipient_total: row.recipient_total,
        accepted_count: row.accepted_count,
        failed_count: row.failed_count,
        created_at: stamp(Some(row.created_at)),
        started_at: stamp(row.started_at),
        completed_at: stamp(row.completed_at),
        next_send_at: stamp(row.next_send_at),
        cancel_requested_at: stamp(row.cancel_requested_at),
        cancel_requested_by_name: row.cancel_requested_by_name,
        error: row.error,
        failures,
    }))
}

fn status_slug(status: ContactMailTaskStatus) -> &'static str {
    match status {
        ContactMailTaskStatus::Queued => "queued",
        ContactMailTaskStatus::Running => "running",
        ContactMailTaskStatus::Cancelling => "cancelling",
        ContactMailTaskStatus::Completed => "completed",
        ContactMailTaskStatus::Cancelled => "cancelled",
        ContactMailTaskStatus::Failed => "failed",
    }
}

fn stamp(value: Option<DateTime<Utc>>) -> String {
    value
        .map(|at| at.with_timezone(&chrono::Local).format(STAMP).to_string())
        .unwrap_or_default()
}
