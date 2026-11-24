//! Contact persistence (SSR only): the people the foundation knows.
//!
//! The account link is the delicate part. `contacts.user_id` is nullable and
//! uniquely indexed, and reads join `users` for the email and role rather than
//! copying them: a contact must never become a second source of truth for who
//! someone is, or a second way to sign in.

use crate::server::db::{audit, contact_properties, ids, pool};
use crate::server_fns::contacts::{
    ActiveContactSummary, Contact, ContactFilters, ContactInput, ContactType, LinkableAccount,
};
use crate::server_fns::pagination::Page;
use crate::server_fns::users::AccountRole;

#[derive(sqlx::FromRow)]
struct ContactRow {
    id: String,
    first_name: String,
    last_name: String,
    preferred_name: String,
    email: String,
    phone: String,
    mobile: String,
    address: String,
    job_title: String,
    organization_id: Option<String>,
    organization_name: Option<String>,
    types: Vec<String>,
    organization_archived: Option<bool>,
    source: String,
    description: String,
    do_not_contact: bool,
    archived: bool,
    account_linked: bool,
    user_id: Option<String>,
    linked_email: Option<String>,
    linked_role: Option<String>,
    has_account_field_conflict: bool,
}

impl From<ContactRow> for Contact {
    fn from(row: ContactRow) -> Self {
        Self {
            id: row.id,
            first_name: row.first_name,
            last_name: row.last_name,
            preferred_name: row.preferred_name,
            email: row.email,
            phone: row.phone,
            mobile: row.mobile,
            address: row.address,
            job_title: row.job_title,
            organization_id: row.organization_id.unwrap_or_default(),
            organization_name: row.organization_name.unwrap_or_default(),
            organization_archived: row.organization_archived.unwrap_or(false),
            // An unknown slug would mean the CHECK constraint was bypassed; drop
            // it rather than failing the whole read.
            types: row
                .types
                .iter()
                .filter_map(|slug| ContactType::from_slug(slug))
                .collect(),
            source: row.source,
            description: row.description,
            do_not_contact: row.do_not_contact,
            archived: row.archived,
            account_linked: row.account_linked,
            user_id: row.user_id.unwrap_or_default(),
            linked_email: row.linked_email.unwrap_or_default(),
            linked_role: row.linked_role.as_deref().and_then(AccountRole::from_slug),
            has_account_field_conflict: row.has_account_field_conflict,
            openable_profile_id: String::new(),
        }
    }
}

#[derive(sqlx::FromRow)]
struct ActiveContactRow {
    id: String,
    label: String,
    organization_id: Option<String>,
    organization_name: Option<String>,
}

impl From<ActiveContactRow> for ActiveContactSummary {
    fn from(row: ActiveContactRow) -> Self {
        Self {
            id: row.id,
            label: row.label,
            organization_id: row.organization_id.unwrap_or_default(),
            organization_name: row.organization_name.unwrap_or_default(),
        }
    }
}

#[derive(sqlx::FromRow)]
struct ContactOrganizationState {
    last_name: String,
    organization_id: Option<String>,
    organization_name: Option<String>,
    display_name: String,
}

#[derive(sqlx::FromRow)]
struct OrganizationRef {
    id: String,
    name: String,
    archived: bool,
}

/// The `SELECT` list every read shares. The organization name and the account's
/// email/role are joined in for display and stay owned by their own tables.
const SELECT_COLUMNS: &str = "c.id,
     CASE WHEN u.id IS NULL THEN c.first_name ELSE u.first_name END AS first_name,
     CASE WHEN u.id IS NULL THEN c.last_name ELSE u.last_name END AS last_name,
     c.preferred_name,
     CASE WHEN u.id IS NULL THEN c.email ELSE u.email END AS email,
     CASE WHEN u.id IS NULL THEN c.phone ELSE u.phone END AS phone,
     c.mobile,
     CASE WHEN u.id IS NULL THEN c.address ELSE u.home_address END AS address,
     c.job_title, c.organization_id,
     o.name AS organization_name, c.types, o.archived AS organization_archived,
     c.source, c.description, c.do_not_contact, c.archived,
     (c.user_id IS NOT NULL) AS account_linked, c.user_id,
     u.email AS linked_email, u.role AS linked_role,
     EXISTS (SELECT 1 FROM contact_account_conflicts conflict
             WHERE conflict.contact_id = c.id) AS has_account_field_conflict";

/// CRM-owned values only: account linkage and private account projections are withheld.
const SAFE_SELECT_COLUMNS: &str = "c.id, c.first_name, c.last_name, c.preferred_name,
     c.email, c.phone, c.mobile, c.address, c.job_title, c.organization_id,
     o.name AS organization_name, c.types, o.archived AS organization_archived,
     c.source, c.description, c.do_not_contact, c.archived,
     (c.user_id IS NOT NULL) AS account_linked,
     NULL::text AS user_id, NULL::text AS linked_email, NULL::text AS linked_role,
     false AS has_account_field_conflict";

const FROM_JOINS: &str = "FROM contacts c
     LEFT JOIN organizations o ON o.id = c.organization_id
     LEFT JOIN users u ON u.id = c.user_id";

const PERSON_NAME_SQL: &str = "nullif(
     btrim(coalesce(nullif(c.preferred_name, ''), c.first_name) || ' ' || c.last_name),
     ''
 )";

const ACTIVE_LABEL_SQL: &str = "coalesce(
     CASE
         WHEN nullif(
             btrim(coalesce(nullif(c.preferred_name, ''), c.first_name) || ' ' || c.last_name),
             ''
         ) IS NULL THEN nullif(o.name, '')
         ELSE btrim(coalesce(nullif(c.preferred_name, ''), c.first_name) || ' ' || c.last_name)
              || coalesce(' (' || nullif(o.name, '') || ')', '')
     END,
     c.id
 )";

fn escaped_like_pattern(query: &str) -> Option<String> {
    let term = query.trim();
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

/// One page of the directory, filtered and searched in SQL so the browser never
/// receives rows it did not ask for.
pub async fn page(
    filters: &ContactFilters,
    offset: i64,
    limit: i64,
    include_account_projection: bool,
) -> Result<Page<Contact>, sqlx::Error> {
    let keyword = filters.keyword.trim();
    let contact_type = filters.contact_type.map(|t| t.slug()).unwrap_or_default();
    let organization_id = filters.organization_id.trim();
    let select_columns = if include_account_projection {
        SELECT_COLUMNS
    } else {
        SAFE_SELECT_COLUMNS
    };

    let where_sql = "WHERE ($1 OR NOT c.archived)
           AND ($2 = '' OR $2 = ANY(c.types))
           AND ($3 = '' OR c.organization_id = $3)
           AND ($4 = '' OR c.first_name ILIKE '%' || $4 || '%'
                        OR c.last_name ILIKE '%' || $4 || '%'
                        OR c.preferred_name ILIKE '%' || $4 || '%'
                        OR c.email ILIKE '%' || $4 || '%'
                        OR c.phone ILIKE '%' || $4 || '%'
                        OR c.mobile ILIKE '%' || $4 || '%'
                        OR o.name ILIKE '%' || $4 || '%')";

    let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) {FROM_JOINS} {where_sql}"))
        .bind(filters.include_archived)
        .bind(contact_type)
        .bind(organization_id)
        .bind(keyword)
        .fetch_one(pool())
        .await?;

    let rows = sqlx::query_as::<_, ContactRow>(&format!(
        "SELECT {select_columns} {FROM_JOINS} {where_sql}
         ORDER BY c.archived ASC, lower(c.last_name) ASC, lower(c.first_name) ASC
         OFFSET $5 LIMIT $6"
    ))
    .bind(filters.include_archived)
    .bind(contact_type)
    .bind(organization_id)
    .bind(keyword)
    .bind(offset.max(0))
    .bind(limit.clamp(1, 200))
    .fetch_all(pool())
    .await?;

    Ok(Page {
        items: rows.into_iter().map(Into::into).collect(),
        total,
    })
}

pub async fn get(
    id: &str,
    include_account_projection: bool,
) -> Result<Option<Contact>, sqlx::Error> {
    let select_columns = if include_account_projection {
        SELECT_COLUMNS
    } else {
        SAFE_SELECT_COLUMNS
    };
    let row = sqlx::query_as::<_, ContactRow>(&format!(
        "SELECT {select_columns} {FROM_JOINS} WHERE c.id = $1"
    ))
    .bind(id)
    .fetch_optional(pool())
    .await?;
    Ok(row.map(Into::into))
}

pub async fn account_of(id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, Option<String>>("SELECT user_id FROM contacts WHERE id = $1")
        .bind(id)
        .fetch_optional(pool())
        .await
        .map(Option::flatten)
}

/// The contact linked to an account, if there is one. Used by the person page to
/// go from a user to their CRM record.
pub async fn for_user(user_id: &str) -> Result<Option<Contact>, sqlx::Error> {
    let row = sqlx::query_as::<_, ContactRow>(&format!(
        "SELECT {SELECT_COLUMNS} {FROM_JOINS} WHERE c.user_id = $1"
    ))
    .bind(user_id)
    .fetch_optional(pool())
    .await?;
    Ok(row.map(Into::into))
}

/// Active contacts for typeahead pickers, with only the fields the picker needs.
pub async fn search_active(
    query: &str,
    limit: i64,
) -> Result<Vec<ActiveContactSummary>, sqlx::Error> {
    let limit = limit.clamp(1, 50);
    let pattern = escaped_like_pattern(query);

    let rows = sqlx::query_as::<_, ActiveContactRow>(&format!(
        "SELECT c.id, {ACTIVE_LABEL_SQL} AS label, c.organization_id, o.name AS organization_name
         FROM contacts c
         LEFT JOIN organizations o ON o.id = c.organization_id
         WHERE NOT c.archived
           AND ($1::text IS NULL
                OR c.first_name ILIKE $1
                OR c.last_name ILIKE $1
                OR c.preferred_name ILIKE $1
                OR {PERSON_NAME_SQL} ILIKE $1
                OR c.email ILIKE $1
                OR c.phone ILIKE $1
                OR c.mobile ILIKE $1
                OR o.name ILIKE $1)
         ORDER BY lower(c.last_name) ASC, lower(c.first_name) ASC, c.seq DESC
         LIMIT $2"
    ))
    .bind(&pattern)
    .bind(limit)
    .fetch_all(pool())
    .await?;

    Ok(rows.into_iter().map(Into::into).collect())
}

fn type_slugs(input: &ContactInput) -> Vec<String> {
    input.types.iter().map(|t| t.slug().to_string()).collect()
}

/// An empty organization id means "not filed under one", which is stored NULL.
fn optional(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

async fn validate_active_organization_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Option<&str>,
) -> Result<Option<OrganizationRef>, sqlx::Error> {
    let Some(organization_id) = organization_id else {
        return Ok(None);
    };
    let organization = sqlx::query_as::<_, OrganizationRef>(
        "SELECT id, name, archived FROM organizations WHERE id = $1 FOR UPDATE",
    )
    .bind(organization_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| sqlx::Error::Protocol("That organization no longer exists.".into()))?;
    if organization.archived {
        return Err(sqlx::Error::Protocol(
            "Archived organizations cannot receive new contacts.".into(),
        ));
    }
    Ok(Some(organization))
}

async fn insert_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
    input: &ContactInput,
    user_id: Option<&str>,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let _ = validate_active_organization_in(tx, optional(&input.organization_id)).await?;
    sqlx::query(
        "INSERT INTO contacts
             (id, first_name, last_name, preferred_name, email, phone, mobile,
              address, job_title, organization_id, user_id, types, source,
              description, do_not_contact)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(id)
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(&input.preferred_name)
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.mobile)
    .bind(&input.address)
    .bind(&input.job_title)
    .bind(optional(&input.organization_id))
    .bind(user_id)
    .bind(type_slugs(input))
    .bind(&input.source)
    .bind(&input.description)
    .bind(input.do_not_contact)
    .execute(&mut **tx)
    .await?;
    contact_properties::add_defaults_for_new_contact(tx, id).await?;
    audit::record_in_transaction(
        tx,
        audit::Entity::Contact,
        id,
        actor,
        "contact",
        "",
        "created",
    )
    .await?;
    if let Some(organization_id) = optional(&input.organization_id) {
        let display_name = format!("{} {}", input.preferred_name, input.last_name)
            .trim()
            .to_string();
        let display_name = if display_name.is_empty() {
            format!("{} {}", input.first_name, input.last_name)
                .trim()
                .to_string()
        } else {
            display_name
        };
        audit::record_in_transaction(
            tx,
            audit::Entity::Organization,
            organization_id,
            actor,
            "person",
            "",
            &format!("linked {display_name}"),
        )
        .await?;
    }
    Ok(())
}

pub async fn create(
    input: &ContactInput,
    actor_user_id: &str,
    actor: &str,
) -> Result<String, sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let id = ids::next(&mut *tx, "ct").await?;
    insert_in(&mut tx, &id, input, None, actor).await?;
    tx.commit().await?;
    Ok(id)
}

pub async fn create_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &ContactInput,
    actor: &str,
) -> Result<String, sqlx::Error> {
    let id = ids::next(&mut **tx, "ct").await?;
    insert_in(tx, &id, input, None, actor).await?;
    Ok(id)
}

/// Create a linked contact inside an existing transaction.
///
/// Used by registration and backfills so the account, its linked contact,
/// default properties, and audit entry all commit together.
pub async fn create_linked_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    input: &ContactInput,
    user_id: &str,
    actor: &str,
) -> Result<String, sqlx::Error> {
    if let Some(existing_id) =
        sqlx::query_scalar::<_, String>("SELECT id FROM contacts WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&mut **tx)
            .await?
    {
        return Ok(existing_id);
    }
    let id = ids::next(&mut **tx, "ct").await?;
    insert_in(tx, &id, input, Some(user_id), actor).await?;
    Ok(id)
}

/// Make sure an account that has just become a volunteer has a contact record
/// carrying the `volunteer` type, creating one when they have none.
///
/// Runs inside the caller's transaction so the role grant and the CRM record
/// commit together, and is idempotent: re-approving the same person changes
/// nothing and writes no audit noise. Returns the contact id, or `None` when the
/// account cannot be represented as a contact.
pub async fn ensure_volunteer_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: &str,
    actor: &str,
) -> Result<Option<String>, sqlx::Error> {
    let volunteer = ContactType::Volunteer.slug();

    let existing = sqlx::query_as::<_, (String, Vec<String>, bool)>(
        "SELECT id, types, archived FROM contacts WHERE user_id = $1 FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;

    if let Some((id, types, archived)) = existing {
        // A full type list is left alone rather than silently dropping one the
        // foundation chose; the same limit the edit form enforces.
        if !types.iter().any(|slug| slug == volunteer)
            && types.len() < crate::server_fns::contacts::MAX_TYPES
        {
            let mut updated = types.clone();
            updated.push(volunteer.to_string());
            sqlx::query("UPDATE contacts SET types = $2, updated_at = now() WHERE id = $1")
                .bind(&id)
                .bind(&updated)
                .execute(&mut **tx)
                .await?;
            audit::record_in_transaction(
                tx,
                audit::Entity::Contact,
                &id,
                actor,
                "types",
                &types.join(", "),
                &updated.join(", "),
            )
            .await?;
        }
        // An approved volunteer is active by definition, so an archived record
        // comes back rather than staying hidden from the directory.
        if archived {
            sqlx::query("UPDATE contacts SET archived = false, updated_at = now() WHERE id = $1")
                .bind(&id)
                .execute(&mut **tx)
                .await?;
            audit::record_in_transaction(
                tx,
                audit::Entity::Contact,
                &id,
                actor,
                "contact",
                "",
                "restored",
            )
            .await?;
        }
        return Ok(Some(id));
    }

    let account = sqlx::query_as::<_, (String, String, String, String, String)>(
        "SELECT first_name, last_name, email, phone, home_address FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((first_name, last_name, email, phone, address)) = account else {
        return Ok(None);
    };
    // Matches `contacts_named_check`: a person with neither surname nor employer
    // has nothing to show, and failing here would block the approval itself.
    if last_name.trim().is_empty() {
        tracing::warn!("no volunteer contact created for {user_id}: the account has no last name");
        return Ok(None);
    }

    let id = ids::next(&mut **tx, "ct").await?;
    insert_in(
        tx,
        &id,
        &ContactInput {
            first_name,
            last_name,
            email,
            phone,
            address,
            types: vec![ContactType::Volunteer],
            source: "Volunteer approval".to_string(),
            ..Default::default()
        },
        Some(user_id),
        actor,
    )
    .await?;
    Ok(Some(id))
}

pub async fn update(
    id: &str,
    input: &ContactInput,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let current = sqlx::query_as::<_, ContactOrganizationState>(&format!(
        "SELECT c.last_name, c.organization_id, o.name AS organization_name,
                coalesce({PERSON_NAME_SQL}, nullif(o.name, ''), c.id) AS display_name
         FROM contacts c
         LEFT JOIN organizations o ON o.id = c.organization_id
         WHERE c.id = $1 FOR UPDATE OF c"
    ))
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;
    if current.organization_id.as_deref() != optional(&input.organization_id) {
        let _ = validate_active_organization_in(&mut tx, optional(&input.organization_id)).await?;
    }
    let updated = sqlx::query(
        "UPDATE contacts
         SET first_name = CASE WHEN user_id IS NULL THEN $2 ELSE first_name END,
             last_name = CASE WHEN user_id IS NULL THEN $3 ELSE last_name END,
             preferred_name = $4,
             email = CASE WHEN user_id IS NULL THEN $5 ELSE email END,
             phone = CASE WHEN user_id IS NULL THEN $6 ELSE phone END,
             mobile = $7,
             address = CASE WHEN user_id IS NULL THEN $8 ELSE address END,
             job_title = $9,
             organization_id = $10, types = $11, source = $12, description = $13,
             do_not_contact = $14, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(&input.first_name)
    .bind(&input.last_name)
    .bind(&input.preferred_name)
    .bind(&input.email)
    .bind(&input.phone)
    .bind(&input.mobile)
    .bind(&input.address)
    .bind(&input.job_title)
    .bind(optional(&input.organization_id))
    .bind(type_slugs(input))
    .bind(&input.source)
    .bind(&input.description)
    .bind(input.do_not_contact)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Contact,
        id,
        actor,
        "contact",
        "",
        "updated",
    )
    .await?;
    let target_id = optional(&input.organization_id);
    if current.organization_id.as_deref() != target_id {
        let old_name = current.organization_name.as_deref().unwrap_or("unassigned");
        let new_name: Option<String> = match target_id {
            Some(target_id) => {
                sqlx::query_scalar("SELECT name FROM organizations WHERE id = $1")
                    .bind(target_id)
                    .fetch_optional(&mut *tx)
                    .await?
            }
            None => None,
        };
        let new_name = new_name.as_deref().unwrap_or("unassigned");
        audit::record_in_transaction(
            &mut tx,
            audit::Entity::Contact,
            id,
            actor,
            "organization",
            old_name,
            new_name,
        )
        .await?;
        if let Some(old_id) = current.organization_id.as_deref() {
            audit::record_in_transaction(
                &mut tx,
                audit::Entity::Organization,
                old_id,
                actor,
                &current.display_name,
                "linked here",
                &format!("moved to {new_name}"),
            )
            .await?;
        }
        if let Some(new_id) = target_id {
            audit::record_in_transaction(
                &mut tx,
                audit::Entity::Organization,
                new_id,
                actor,
                &current.display_name,
                old_name,
                "linked here",
            )
            .await?;
        }
    }
    tx.commit().await
}

pub async fn set_archived(
    id: &str,
    archived: bool,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let updated =
        sqlx::query("UPDATE contacts SET archived = $2, updated_at = now() WHERE id = $1")
            .bind(id)
            .bind(archived)
            .execute(&mut *tx)
            .await?;
    if updated.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Contact,
        id,
        actor,
        "contact",
        "",
        if archived { "archived" } else { "restored" },
    )
    .await?;
    tx.commit().await
}

/// Accounts that no contact points at yet, newest surname order.
pub async fn unlinked_accounts() -> Result<Vec<LinkableAccount>, sqlx::Error> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT u.id, btrim(u.first_name || ' ' || u.last_name), u.email
         FROM users u
         WHERE NOT EXISTS (SELECT 1 FROM contacts c WHERE c.user_id = u.id)
           AND u.role <> 'deactivated'
         ORDER BY lower(u.last_name) ASC, lower(u.first_name) ASC",
    )
    .fetch_all(pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, email)| LinkableAccount { id, name, email })
        .collect())
}

/// Link this contact to an account, or unlink it when `user_id` is `None`.
///
/// Only ever writes `contacts.user_id`: the account itself is untouched, so
/// unlinking leaves the person able to sign in exactly as before.
pub async fn set_account(
    id: &str,
    user_id: Option<&str>,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let updated = sqlx::query("UPDATE contacts SET user_id = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Contact,
        id,
        actor,
        "account link",
        "",
        if user_id.is_some() {
            "linked"
        } else {
            "unlinked"
        },
    )
    .await?;
    tx.commit().await
}

/// Change the one canonical organization link on a contact.
pub async fn set_organization(
    contact_id: &str,
    organization_id: Option<&str>,
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    audit::set_actor_in_transaction(&mut tx, actor_user_id).await?;
    let current = sqlx::query_as::<_, ContactOrganizationState>(&format!(
        "SELECT c.last_name, c.organization_id, o.name AS organization_name,
                    coalesce({PERSON_NAME_SQL}, nullif(o.name, ''), c.id) AS display_name
             FROM contacts c
             LEFT JOIN organizations o ON o.id = c.organization_id
             WHERE c.id = $1
             FOR UPDATE OF c"
    ))
    .bind(contact_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;

    let target = validate_active_organization_in(&mut tx, organization_id).await?;
    if current.organization_id.as_deref() == target.as_ref().map(|org| org.id.as_str()) {
        return Ok(());
    }
    if target.is_none() && current.last_name.trim().is_empty() {
        return Err(sqlx::Error::Protocol(
            "This contact needs a last name before you can remove their organization.".into(),
        ));
    }

    sqlx::query("UPDATE contacts SET organization_id = $2, updated_at = now() WHERE id = $1")
        .bind(contact_id)
        .bind(target.as_ref().map(|org| org.id.as_str()))
        .execute(&mut *tx)
        .await?;

    let old_name = current.organization_name.clone().unwrap_or_default();
    let new_name = target
        .as_ref()
        .map(|org| org.name.clone())
        .unwrap_or_default();
    let old_display = if old_name.is_empty() {
        "unassigned".to_string()
    } else {
        old_name.clone()
    };
    let new_display = if new_name.is_empty() {
        "unassigned".to_string()
    } else {
        new_name.clone()
    };
    audit::record_in_transaction(
        &mut tx,
        audit::Entity::Contact,
        contact_id,
        actor,
        "organization",
        &old_display,
        &new_display,
    )
    .await?;

    if let Some(old_org_id) = current.organization_id.as_deref() {
        let destination = target
            .as_ref()
            .map(|org| format!("moved to {}", org.name))
            .unwrap_or_else(|| "removed".to_string());
        audit::record_in_transaction(
            &mut tx,
            audit::Entity::Organization,
            old_org_id,
            actor,
            &current.display_name,
            "linked here",
            &destination,
        )
        .await?;
    }

    if let Some(new_org) = &target {
        audit::record_in_transaction(
            &mut tx,
            audit::Entity::Organization,
            &new_org.id,
            actor,
            &current.display_name,
            &old_display,
            "linked here",
        )
        .await?;
    }

    tx.commit().await
}
