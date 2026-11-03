//! Cases, sub properties of case_properties are their own files

use crate::server::db::{
    audit, capabilities, case_documents, case_notes, case_properties, channels, client_agreements,
    ids, now_stamp, pool, terms_acceptances, users,
};
use crate::server_fns::capabilities::CaseCapability;
use crate::server_fns::case_properties::CaseProperty;
use crate::server_fns::cases::{Case, CaseStatus, CaseSummary, CaseWithdrawal};
use crate::server_fns::channels::ChannelKind;
use crate::server_fns::pagination::Page;
use crate::server_fns::users::{AccountRole, User};

#[derive(sqlx::FromRow)]
struct CaseRow {
    id: String,
    name: String,
    status: String,
    review_reason: String,
    owner_id: String,
    withdrawn_by: String,
    withdrawn_at: String,
    withdrawal_reason: String,
}

/// Flat row shape for the sparse [`CaseSummary`] projection (header fields plus
/// resolved owner name and message count). Shared by every summary query.
#[derive(sqlx::FromRow)]
struct SummaryRow {
    id: String,
    name: String,
    status: String,
    review_reason: String,
    owner_id: String,
    owner_first_name: String,
    owner_last_name: String,
    message_count: i64,
    last_activity: Option<String>,
}

impl SummaryRow {
    fn into_summary(self, capabilities: Vec<CaseCapability>, threshold: &str) -> CaseSummary {
        let inactive = self
            .last_activity
            .as_deref()
            .is_some_and(|ts| ts < threshold);
        CaseSummary {
            id: self.id,
            name: self.name,
            status: CaseStatus::from_slug(&self.status).unwrap_or(CaseStatus::Open),
            review_reason: self.review_reason,
            owner_id: self.owner_id,
            owner_first_name: self.owner_first_name,
            owner_last_name: self.owner_last_name,
            message_count: self.message_count.max(0) as usize,
            inactive,
            capabilities,
        }
    }
}

/// The `YYYY-MM-DD HH:MM` local-time cutoff (30 days ago) for the case-chat
/// inactivity badge, matching the format [`now_stamp`] writes for `sent_at`.
fn inactivity_threshold() -> String {
    (chrono::Local::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// The `SELECT` list that projects a `cases` row (aliased `c`) into a
/// [`SummaryRow`]: header fields, the owner's first/last name, and the case's
/// message count. Callers append their own `WHERE`/`ORDER`/`LIMIT`.
///
/// `message_count_scope` is an SQL boolean spliced into the count subquery that
/// decides whether the volunteer-only channel's messages are included. Passing
/// `"true"` counts everything (site-admin-only lookups that never surface to a
/// client); a viewer-scoped query passes a role test so a client's case list
/// does not even leak *how many* private staff messages exist. It is always an
/// internal constant or a bind-parameter reference, never user input.
fn summary_select(message_count_scope: &str) -> String {
    format!(
        "SELECT c.id, c.name, c.status, c.review_reason, c.owner_id,
        u.first_name AS owner_first_name,
        u.last_name AS owner_last_name,
        (SELECT COUNT(*) FROM messages m
          JOIN case_channels ch ON ch.id = m.channel_id
          WHERE m.case_id = c.id
            AND (({message_count_scope}) OR ch.kind <> '{restricted}')) AS message_count,
        (SELECT MAX(m.sent_at) FROM messages m
          JOIN case_channels ch ON ch.id = m.channel_id
          WHERE m.case_id = c.id
            AND (({message_count_scope}) OR ch.kind <> '{restricted}')) AS last_activity
 FROM cases c
 LEFT JOIN users u ON u.id = c.owner_id",
        restricted = ChannelKind::VolunteerOnly.slug()
    )
}

/// Escape a free-text term for a literal `%term%` ILIKE match, or `None` for an
/// empty filter.
fn escaped_like_pattern(search: &str) -> Option<String> {
    let term = search.trim();
    if term.is_empty() {
        None
    } else {
        Some(format!(
            "%{}%",
            term.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        ))
    }
}

/// One page of case summaries for the admin directory, ordered by id with an
/// optional case-insensitive search over case id, case name, or owner name.
pub async fn admin_page(
    offset: i64,
    limit: i64,
    search: &str,
    user: &User,
) -> Result<Page<CaseSummary>, sqlx::Error> {
    let limit = limit.clamp(1, 1_000);
    let offset = offset.max(0);
    let pattern = escaped_like_pattern(search);

    const SEARCH: &str = "($1::text IS NULL OR c.id ILIKE $1 OR c.name ILIKE $1 \
        OR COALESCE(u.first_name, '') ILIKE $1 OR COALESCE(u.last_name, '') ILIKE $1 \
        OR concat_ws(' ', COALESCE(u.first_name, ''), COALESCE(u.last_name, '')) ILIKE $1)";
    let count_sql = format!(
        "SELECT count(*) FROM cases c LEFT JOIN users u ON u.id = c.owner_id WHERE {SEARCH}"
    );
    let page_sql = format!(
        "{} WHERE {SEARCH} ORDER BY c.id LIMIT $2 OFFSET $3",
        // Admin-only listing, so count every message like the other admin lookups.
        summary_select("true")
    );

    let count_fut = sqlx::query_scalar::<_, i64>(&count_sql)
        .bind(&pattern)
        .fetch_one(pool());
    let rows_fut = sqlx::query_as::<_, SummaryRow>(&page_sql)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool());
    let (total, rows) = tokio::try_join!(count_fut, rows_fut)?;

    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut capabilities_by_case = if ids.is_empty() {
        Default::default()
    } else {
        capabilities::get_multi_case(user, &ids).await?
    };
    let threshold = inactivity_threshold();
    let items = rows
        .into_iter()
        .map(|r| {
            let caps = capabilities_by_case.remove(&r.id).unwrap_or_default();
            r.into_summary(caps, &threshold)
        })
        .collect();
    Ok(Page { items, total })
}

/// The owner id of a case, if it exists (cheap authorization lookup).
pub async fn owner_id(case_id: &str) -> Result<Option<String>, sqlx::Error> {
    let owner: Option<String> = sqlx::query_scalar("SELECT owner_id FROM cases WHERE id = $1")
        .bind(case_id)
        .fetch_optional(pool())
        .await?;
    Ok(owner)
}

/// The display name of a case, if it exists (cheap lookup for notifications).
pub async fn name(case_id: &str) -> Result<Option<String>, sqlx::Error> {
    let name: Option<String> = sqlx::query_scalar("SELECT name FROM cases WHERE id = $1")
        .bind(case_id)
        .fetch_optional(pool())
        .await?;
    Ok(name)
}

/// One page of sparse cases visible to a user.
///
/// Withdrawn cases are never included. This backs the personal "my cases" list
/// and the Inbox, and withdrawing is precisely how someone takes a case off
/// them. Admins find withdrawn cases through [`page`], the unfiltered admin
/// directory, which is also where they restore one.
pub async fn get_summaries_for_user(
    offset: i64,
    limit: i64,
    search: &str,
    user: &User,
) -> Result<Page<CaseSummary>, sqlx::Error> {
    let limit = limit.clamp(1, 100);
    let offset = offset.max(0);

    // Escaped `%term%` pattern (or `None` for "no filter"), so user input is
    // matched literally rather than as LIKE metacharacters.
    let pattern = escaped_like_pattern(search);

    // `$1` is the (nullable) search pattern; the viewer id scopes the results to
    // cases they can actually open — i.e. where they hold the `view_case`
    // capability, matching what [`load_case`] enforces. Its bind position differs
    // between the COUNT query (`$2`) and the page query (`$4`, after LIMIT/OFFSET).
    // The capability slug is an internal constant (never user input), so
    // interpolating it into the SQL is safe.
    const SEARCH: &str = "($1::text IS NULL OR c.id ILIKE $1 OR c.name ILIKE $1)";
    let scope = format!(
        "EXISTS (SELECT 1 FROM case_assignments a \
         WHERE a.case_id = c.id AND a.user_id = $VIEWER AND a.capability = '{}')",
        CaseCapability::ViewCase.slug()
    );
    // Drops withdrawn cases out of both this list and the Inbox, which share this
    // query. The status slug is an internal constant, never user input.
    let withdrawn = format!(" AND c.status <> '{}'", CaseStatus::Withdrawn.slug());

    let count_sql = format!(
        "SELECT count(*) FROM cases c WHERE {SEARCH} AND {}{withdrawn}",
        scope.replace("$VIEWER", "$2")
    );
    // The viewer's own account role decides whether the volunteer-only channel
    // counts toward the message total they see; clients are never told it exists.
    let page_sql = format!(
        "{}
         WHERE {SEARCH} AND {}{withdrawn}
         ORDER BY c.id LIMIT $2 OFFSET $3",
        summary_select(&format!(
            "(SELECT v.role FROM users v WHERE v.id = $4) IN {}",
            AccountRole::STAFF_ROLES_SQL
        )),
        scope.replace("$VIEWER", "$4")
    );

    // The count and the page fetch are independent, so run them concurrently —
    // one fewer sequential round-trip against a networked database.
    let count_fut = sqlx::query_scalar::<_, i64>(&count_sql)
        .bind(&pattern)
        .bind(&user.id)
        .fetch_one(pool());
    let rows_fut = sqlx::query_as::<_, SummaryRow>(&page_sql)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .bind(&user.id)
        .fetch_all(pool());
    let (total, rows) = tokio::try_join!(count_fut, rows_fut)?;

    // Resolve the requesting user's capabilities on this page of cases in one
    // batched query, then build each summary complete with its rights attached —
    // so the client can gate actions straight from the case data.
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut capabilities_by_case = if ids.is_empty() {
        Default::default()
    } else {
        capabilities::get_multi_case(user, &ids).await?
    };
    let threshold = inactivity_threshold();
    let items = rows
        .into_iter()
        .map(|r| {
            let caps = capabilities_by_case.remove(&r.id).unwrap_or_default();
            r.into_summary(caps, &threshold)
        })
        .collect::<Vec<_>>();
    Ok(Page { items, total })
}

/// One admin directory summary by primary key, including the viewer's
/// capabilities. This avoids resolving detail routes through fuzzy search.
pub async fn admin_summary(case_id: &str, user: &User) -> Result<Option<CaseSummary>, sqlx::Error> {
    let row =
        sqlx::query_as::<_, SummaryRow>(&format!("{} WHERE c.id = $1", summary_select("true")))
            .bind(case_id)
            .fetch_optional(pool())
            .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let caps = capabilities::get_single_case(user, case_id).await?;
    Ok(Some(row.into_summary(caps, &inactivity_threshold())))
}

/// Up to `limit` lightweight cases whose id or name matches `search`
/// (case-insensitive, metacharacters escaped), ordered by name. Unscoped — used by
/// the admin capability tool to find any case to grant access to. Empty search
/// returns the first `limit` cases.
pub async fn search_lite(search: &str, limit: i64) -> Result<Vec<CaseSummary>, sqlx::Error> {
    let limit = limit.clamp(1, 500);
    let pattern = escaped_like_pattern(search);
    let rows = sqlx::query_as::<_, SummaryRow>(&format!(
        "{}
         WHERE $1::text IS NULL OR c.id ILIKE $1 OR c.name ILIKE $1
         ORDER BY lower(c.name), c.id LIMIT $2",
        summary_select("true")
    ))
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool())
    .await?;
    let threshold = inactivity_threshold();
    Ok(rows
        .into_iter()
        .map(|r| r.into_summary(Vec::new(), &threshold))
        .collect())
}

/// Cases the caller may edit, for the person-side relationship picker. A role
/// with full case access may edit every live case, so its scope clause is
/// simply true.
pub async fn search_editable(
    search: &str,
    user: &User,
    limit: i64,
) -> Result<Vec<crate::server_fns::case_contacts::EditableCaseSummary>, sqlx::Error> {
    let pattern = escaped_like_pattern(search);
    // The capability slug is an internal constant, never user input. The
    // full-access branch still references `$2` (the viewer id) so the statement
    // keeps the same parameter list either way.
    let scope = if user.role.has_full_case_access() {
        "$2::text IS NOT NULL"
    } else {
        "EXISTS (
               SELECT 1 FROM case_assignments assignment
               WHERE assignment.case_id = c.id
                 AND assignment.user_id = $2
                 AND assignment.capability = 'edit_case'
           )"
    };
    let rows: Vec<(String, String, String)> = sqlx::query_as(&format!(
        "SELECT c.id, c.name, c.status
         FROM cases c
         WHERE c.status <> 'declined'
           AND c.status <> 'withdrawn'
           AND {scope}
           AND ($1::text IS NULL OR c.id ILIKE $1 OR c.name ILIKE $1)
         ORDER BY c.id LIMIT $3"
    ))
    .bind(&pattern)
    .bind(&user.id)
    .bind(limit.clamp(1, 50))
    .fetch_all(pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, name, status)| crate::server_fns::case_contacts::EditableCaseSummary {
                id,
                name,
                status: CaseStatus::from_slug(&status).unwrap_or(CaseStatus::Open),
            },
        )
        .collect())
}

/// The cases awaiting an accept/decline decision. Callers gate on admin.
pub async fn pending_review_cases() -> Result<Vec<CaseSummary>, sqlx::Error> {
    let rows = sqlx::query_as::<_, SummaryRow>(&format!(
        "{} WHERE c.status = $1 ORDER BY c.id",
        // Admin-only listing, so count every message like the other admin lookups.
        summary_select("true")
    ))
    .bind(CaseStatus::PendingReview.slug())
    .fetch_all(pool())
    .await?;
    let threshold = inactivity_threshold();
    Ok(rows
        .into_iter()
        .map(|r| r.into_summary(Vec::new(), &threshold))
        .collect())
}

/// How many cases are waiting for an admin decision.
pub async fn pending_review_count() -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM cases WHERE status = $1")
        .bind(CaseStatus::PendingReview.slug())
        .fetch_one(pool())
        .await
}

/// Lightweight [`CaseSummary`]s for a specific set of ids, ordered by id. Used to
/// resolve case names for display without loading every case. Empty input
/// returns an empty vec (no query).
pub async fn get_summaries_by_ids(ids: &[String]) -> Result<Vec<CaseSummary>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query_as::<_, SummaryRow>(&format!(
        "{} WHERE c.id = ANY($1) ORDER BY c.id",
        summary_select("true")
    ))
    .bind(ids)
    .fetch_all(pool())
    .await?;
    let threshold = inactivity_threshold();
    Ok(rows
        .into_iter()
        .map(|r| r.into_summary(Vec::new(), &threshold))
        .collect())
}

/// A single case by id, fully hydrated (properties, its newest notes, and where
/// its documents live) including the capabilities `user` holds on it, or `None`
/// if no such case exists.
pub async fn get(
    id: &str,
    user: &User,
    has_volunteer_access: bool,
) -> Result<Option<Case>, sqlx::Error> {
    let Some(row) = sqlx::query_as::<_, CaseRow>(
        "SELECT id, name, status, review_reason, owner_id,
                withdrawn_by, withdrawn_at, withdrawal_reason
           FROM cases WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool())
    .await?
    else {
        return Ok(None);
    };

    // Keep the legacy case-detail field limited to migrated shared free-text
    // notes. Structured staff-only notes are loaded through `case_notes` APIs.
    let notes = case_notes::legacy_notes_for_case(id).await?;

    // Where this case's files live. The listing itself is fetched separately by
    // the documents panel, so opening a case never waits on the library.
    let documents = case_documents::folder_ref(id).await?;
    let properties = case_properties::get_case_properties(id, has_volunteer_access).await?;
    let signer = client_agreements::signer_for_case(id).await?;
    let terms_accepted = terms_acceptances::for_case(id).await?.map(|acceptance| {
        let when = format!(
            "{} (version {})",
            acceptance
                .accepted_at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M"),
            acceptance.terms_version
        );
        // Who signed it electronically, when the agreement recorded a signature.
        match signer {
            Some(name) => format!("{when}, signed by {name}"),
            None => when,
        }
    });

    let status = CaseStatus::from_slug(&row.status).unwrap_or(CaseStatus::Open);
    // Only a withdrawn case carries this; the columns are empty otherwise.
    let withdrawal = (status == CaseStatus::Withdrawn).then(|| CaseWithdrawal {
        by: row.withdrawn_by,
        at: row.withdrawn_at,
        reason: row.withdrawal_reason,
    });

    Ok(Some(Case {
        id: row.id,
        name: row.name,
        status,
        review_reason: row.review_reason,
        owner_id: row.owner_id,
        notes,
        documents_ready: documents.is_ready(),
        documents_web_url: documents.web_url,
        properties,
        message_count: 0,
        capabilities: capabilities::get_single_case(user, id).await?,
        terms_accepted,
        withdrawal,
    }))
}

/// Create a case owned by `owner_id` with an initial status and cleaned property
/// set. Grants the owner full capabilities and returns the new case id.
pub async fn create(
    owner_id: &str,
    owner_name: &str,
    name: &str,
    status: CaseStatus,
    initial_properties: Vec<CaseProperty>,
) -> Result<String, sqlx::Error> {
    let id = ids::next(pool(), "c").await?;

    let mut tx = pool().begin().await?;
    sqlx::query("INSERT INTO cases (id, name, status, owner_id) VALUES ($1, $2, $3, $4)")
        .bind(&id)
        .bind(name)
        .bind(status.slug())
        .bind(owner_id)
        .execute(&mut *tx)
        .await?;

    // Every case starts with its permanent volunteer-only back-channel and a
    // "General" channel, created in the same transaction so a case can never
    // exist without somewhere to chat.
    channels::create_defaults(&mut *tx, &id).await?;

    // The fields every case starts with come first, so the standing paperwork
    // sits above whatever the creator typed in.
    case_properties::add_for_new_case(&mut tx, &id, case_properties::clean(initial_properties))
        .await?;

    users::assign_capabilities_in(&mut tx, owner_id, &id, &CaseCapability::ALL, owner_name).await?;
    // The case's own creation belongs in its history: without this the Change
    // Log's earliest entry is a change to a case that never appears to have been
    // created. Also what the admin activity feed reads for "new case".
    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Case,
        &id,
        owner_name,
        "case",
        "",
        "created",
    )
    .await?;
    tx.commit().await?;

    // The case's folder in the document library. Deliberately *after* the
    // commit: it is a network call, which has no business inside a transaction,
    // and an unreachable library must not stop a case from being created. A
    // case left without a folder is picked up by the startup backfill or the
    // "Set up documents folder" button.
    provision_documents_for(&id);

    Ok(id)
}

/// Create a case's document folder and sharing invitations in the background.
///
/// Best-effort by design, like the notification helpers: failures are logged and
/// the case is still perfectly usable without its folder. Public so the signup
/// path, which commits its own transaction, can call it at the same point.
pub fn provision_documents_for(case_id: &str) {
    let case_id = case_id.to_string();
    tokio::spawn(async move {
        match crate::server::sharepoint::sync::ensure_case_folder(&case_id).await {
            Ok(_) => crate::server::sharepoint::sync_case_access(case_id),
            Err(e) => tracing::warn!("could not create the document folder for {case_id}: {e}"),
        }
    });
}

/// Create the case a verified public signup asked for, inside the same
/// transaction that creates the account.
pub async fn create_from_signup_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    case_id: &str,
    owner_id: &str,
    owner_name: &str,
    name: &str,
    initial_properties: Vec<CaseProperty>,
    terms_version: &str,
) -> Result<(), sqlx::Error> {
    // Signup cases have had no staff involvement, so they start awaiting a decision.
    sqlx::query("INSERT INTO cases (id, name, status, owner_id) VALUES ($1, $2, $3, $4)")
        .bind(case_id)
        .bind(name)
        .bind(CaseStatus::PendingReview.slug())
        .bind(owner_id)
        .execute(&mut **tx)
        .await?;
    channels::create_defaults(&mut **tx, case_id).await?;
    case_properties::add_for_new_case(tx, case_id, case_properties::clean(initial_properties))
        .await?;
    terms_acceptances::insert_in(tx, owner_id, Some(case_id), terms_version).await?;
    users::assign_capabilities_in(tx, owner_id, case_id, &CaseCapability::ALL, owner_name).await?;
    // Same as [`create`]: a signup's case is created too, and the feed reads it.
    audit::record_in_transaction(
        tx,
        audit::Entity::Case,
        case_id,
        owner_name,
        "case",
        "",
        "created from a client signup",
    )
    .await?;
    Ok(())
}

/// Update a single `cases` column and record an audit entry — but only when the
/// case exists and the value actually changes. `column` and `field` are always
/// internal string constants (never caller/user input), so interpolating the
/// column name into the SQL is safe. `old_display`/`new_display` are the
/// human-readable values written to the audit log, which may differ from the
/// stored value (e.g. owner ids stored, owner names audited).
async fn update_field(
    case_id: &str,
    column: &str,
    new_stored: &str,
    current_stored: &str,
    actor: &str,
    field: &str,
    old_display: &str,
    new_display: &str,
) -> Result<(), sqlx::Error> {
    if current_stored == new_stored {
        return Ok(());
    }
    sqlx::query(&format!("UPDATE cases SET {column} = $1 WHERE id = $2"))
        .bind(new_stored)
        .bind(case_id)
        .execute(pool())
        .await?;
    audit::record(
        pool(),
        audit::Entity::Case,
        case_id,
        actor,
        field,
        old_display,
        new_display,
    )
    .await
}

/// Current stored value of a single `cases` column, or `None` if the case is
/// missing. `column` is always an internal constant, never user input.
async fn current_field(case_id: &str, column: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(&format!("SELECT {column} FROM cases WHERE id = $1"))
        .bind(case_id)
        .fetch_optional(pool())
        .await
}

/// Update a case's status, auditing the change.
pub async fn set_status(case_id: &str, status: CaseStatus, actor: &str) -> Result<(), sqlx::Error> {
    let Some(current) = current_field(case_id, "status").await? else {
        return Ok(());
    };
    update_field(
        case_id,
        "status",
        status.slug(),
        &current,
        actor,
        "status",
        &current,
        status.slug(),
    )
    .await
}

/// The current status of a case, or `None` if it does not exist.
pub async fn status(case_id: &str) -> Result<Option<CaseStatus>, sqlx::Error> {
    Ok(current_field(case_id, "status")
        .await?
        .and_then(|s| CaseStatus::from_slug(&s)))
}

/// Record a decision: status, reason, decider, and timestamp in one statement.
pub async fn set_status_with_reason(
    case_id: &str,
    status: CaseStatus,
    reason: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let Some(current) = current_field(case_id, "status").await? else {
        return Ok(());
    };
    sqlx::query(
        "UPDATE cases
            SET status = $1, review_reason = $2, reviewed_by = $3, reviewed_at = $4
          WHERE id = $5",
    )
    .bind(status.slug())
    .bind(reason)
    .bind(actor)
    .bind(now_stamp())
    .bind(case_id)
    .execute(pool())
    .await?;
    audit::record(
        pool(),
        audit::Entity::Case,
        case_id,
        actor,
        "status",
        &current,
        status.slug(),
    )
    .await
}

/// Rename a case, auditing the change.
pub async fn set_name(case_id: &str, name: &str, actor: &str) -> Result<(), sqlx::Error> {
    let Some(current) = current_field(case_id, "name").await? else {
        return Ok(());
    };
    update_field(
        case_id, "name", name, &current, actor, "name", &current, name,
    )
    .await
}

/// Change a case's owner. `owner_id` must reference an existing user. Audits
/// using display names for readability.
pub async fn set_owner(case_id: &str, owner_id: &str, actor: &str) -> Result<(), sqlx::Error> {
    let Some(current) = current_field(case_id, "owner_id").await? else {
        return Ok(());
    };
    if current == owner_id {
        return Ok(());
    }
    let old_display = user_name(&current).await?;
    let new_display = user_name(owner_id).await?;
    update_field(
        case_id,
        "owner_id",
        owner_id,
        &current,
        actor,
        "owner",
        &old_display,
        &new_display,
    )
    .await
}

/// A refusal the caller should show verbatim, rather than a database fault.
/// Mirrors the helper in [`crate::server::db::case_notes`].
fn domain_error(message: &str) -> sqlx::Error {
    sqlx::Error::Protocol(message.to_string())
}

/// Take a case back on the owner's behalf, recording who did it and what status
/// to restore it to. Rejects a case that is already finished or already
/// withdrawn.
///
/// The status is read `FOR UPDATE` inside the transaction that writes it, so two
/// concurrent requests cannot both withdraw the same case and clobber
/// `status_before_withdrawal`.
pub async fn withdraw(case_id: &str, actor: &str, reason: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    let current: Option<String> =
        sqlx::query_scalar("SELECT status FROM cases WHERE id = $1 FOR UPDATE")
            .bind(case_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(current) = current else {
        return Err(domain_error("Case not found."));
    };
    let status = CaseStatus::from_slug(&current).unwrap_or(CaseStatus::Open);
    if !status.can_be_withdrawn() {
        let message = if status == CaseStatus::Withdrawn {
            "This case has already been withdrawn.".to_string()
        } else {
            format!(
                "This case is {} and cannot be withdrawn.",
                status.label().to_lowercase()
            )
        };
        return Err(domain_error(&message));
    }

    sqlx::query(
        "UPDATE cases
            SET status = $1, status_before_withdrawal = $2,
                withdrawn_by = $3, withdrawn_at = $4, withdrawal_reason = $5
          WHERE id = $6",
    )
    .bind(CaseStatus::Withdrawn.slug())
    .bind(&current)
    .bind(actor)
    .bind(now_stamp())
    .bind(reason)
    .bind(case_id)
    .execute(&mut *tx)
    .await?;

    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Case,
        case_id,
        actor,
        "status",
        &current,
        CaseStatus::Withdrawn.slug(),
    )
    .await?;
    tx.commit().await
}

/// Put a withdrawn case back exactly where it was, clearing the withdrawal
/// metadata. Callers gate on operations admin.
pub async fn restore(case_id: &str, actor: &str) -> Result<CaseStatus, sqlx::Error> {
    let mut tx = pool().begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT status, status_before_withdrawal FROM cases WHERE id = $1 FOR UPDATE",
    )
    .bind(case_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((current, previous)) = row else {
        return Err(domain_error("Case not found."));
    };
    if CaseStatus::from_slug(&current) != Some(CaseStatus::Withdrawn) {
        return Err(domain_error("This case is not withdrawn."));
    }
    // The migration's CHECK guarantees a non-empty previous status, so the
    // fallback only covers an unrecognized slug.
    let restored = CaseStatus::from_slug(&previous).unwrap_or(CaseStatus::PendingReview);

    sqlx::query(
        "UPDATE cases
            SET status = $1, status_before_withdrawal = '',
                withdrawn_by = '', withdrawn_at = '', withdrawal_reason = ''
          WHERE id = $2",
    )
    .bind(restored.slug())
    .bind(case_id)
    .execute(&mut *tx)
    .await?;

    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Case,
        case_id,
        actor,
        "status",
        &current,
        restored.slug(),
    )
    .await?;
    tx.commit().await?;
    Ok(restored)
}

/// Display name for a user id, falling back to the id.
async fn user_name(user_id: &str) -> Result<String, sqlx::Error> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT first_name, last_name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool())
            .await?;
    Ok(row
        .map(|(f, l)| format!("{f} {l}").trim().to_string())
        .unwrap_or_else(|| user_id.to_string()))
}
