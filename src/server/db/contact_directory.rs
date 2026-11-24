//! Persistence for the outreach contact directory (SSR only): the reusable
//! classification taxonomy, keyword search, and the communication log.
//!
//! The person record is not duplicated here. A directory row *is* a
//! [`crate::server::db::contacts`] row, read through a flattened projection and
//! written back through that module, so audit entries, default properties, and
//! the account link keep working exactly as they do in Admin.

use std::collections::HashMap;

use crate::server::db::{audit, contact_rules, contacts, ids, organizations, pool, property_filters};
use crate::server_fns::contact_directory::{
    CommunicationKind, Contact, ContactCategory, ContactCommunication, ContactDetails, ContactInput,
    ContactViewSummary,
};
use crate::server_fns::contacts::{ContactInput as PersonInput, ContactType};
use crate::server_fns::organizations::{OrganizationInput, OrganizationKind};
use crate::server_fns::pagination::Page;
use crate::server_fns::property_filters::{PropertyFilter, PropertySubject};

const STAMP: &str = "%Y-%m-%d %H:%M";

/// Marks the people this directory created, so their origin is visible in Admin.
const DIRECTORY_SOURCE: &str = "Contact directory";

/// The directory's flat projection of a person: the account owns identity for
/// linked contacts, so those columns are read from `users` just as the Admin
/// directory reads them.
const DIRECTORY_COLUMNS: &str = "c.id,
     coalesce(
         nullif(
             btrim(
                 coalesce(nullif(c.preferred_name, ''),
                          CASE WHEN u.id IS NULL THEN c.first_name ELSE u.first_name END)
                 || ' '
                 || CASE WHEN u.id IS NULL THEN c.last_name ELSE u.last_name END
             ),
             ''
         ),
         o.name,
         ''
     ) AS full_name,
     c.job_title AS title,
     coalesce(o.name, '') AS organization,
     CASE WHEN u.id IS NULL THEN c.email ELSE u.email END AS email,
     CASE WHEN u.id IS NULL THEN c.phone ELSE u.phone END AS phone,
     CASE WHEN u.id IS NULL THEN c.address ELSE u.home_address END AS address,
     c.website, c.types, c.archived, (u.id IS NOT NULL) AS has_account, c.updated_at,
     category.id AS category_id, category.name AS category_name,
     category.parent_id, coalesce(parent.name, '') AS parent_name";

const SAFE_DIRECTORY_COLUMNS: &str = "c.id,
     coalesce(
         nullif(btrim(coalesce(nullif(c.preferred_name, ''), c.first_name) || ' ' || c.last_name), ''),
         o.name, ''
     ) AS full_name,
     c.job_title AS title, coalesce(o.name, '') AS organization,
     c.email, c.phone, c.address, c.website, c.types, c.archived,
     (u.id IS NOT NULL) AS has_account, c.updated_at,
     category.id AS category_id, category.name AS category_name,
     category.parent_id, coalesce(parent.name, '') AS parent_name";

/// The name the directory displays, composed the same way the projection does so
/// a two-word query matches what the reader sees.
const DISPLAY_NAME_SQL: &str = "btrim(
     coalesce(nullif(c.preferred_name, ''),
              CASE WHEN u.id IS NULL THEN c.first_name ELSE u.first_name END)
     || ' '
     || CASE WHEN u.id IS NULL THEN c.last_name ELSE u.last_name END
 )";

const SAFE_DISPLAY_NAME_SQL: &str =
    "btrim(coalesce(nullif(c.preferred_name, ''), c.first_name) || ' ' || c.last_name)";

const DIRECTORY_JOINS: &str = "FROM contacts c
     LEFT JOIN organizations o ON o.id = c.organization_id
     LEFT JOIN users u ON u.id = c.user_id
     LEFT JOIN contact_category_assignments assignment ON assignment.contact_id = c.id
     LEFT JOIN contact_categories category ON category.id = assignment.category_id
     LEFT JOIN contact_categories parent ON parent.id = category.parent_id";

#[derive(sqlx::FromRow)]
struct CategoryRow {
    id: String,
    name: String,
    parent_id: Option<String>,
    parent_name: String,
}

impl From<CategoryRow> for ContactCategory {
    fn from(row: CategoryRow) -> Self {
        Self {
            id: row.id,
            name: row.name,
            parent_id: row.parent_id,
            parent_name: row.parent_name,
        }
    }
}

#[derive(sqlx::FromRow)]
struct ContactRow {
    id: String,
    full_name: String,
    title: String,
    organization: String,
    email: String,
    phone: String,
    address: String,
    website: String,
    types: Vec<String>,
    archived: bool,
    has_account: bool,
    updated_at: chrono::DateTime<chrono::Utc>,
    category_id: Option<String>,
    category_name: Option<String>,
    parent_id: Option<String>,
    parent_name: Option<String>,
}

fn stamp(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&chrono::Local).format(STAMP).to_string()
}

fn fold_contacts(rows: Vec<ContactRow>) -> Vec<Contact> {
    let mut contacts = Vec::<Contact>::new();
    let mut indexes = HashMap::<String, usize>::new();
    for row in rows {
        let index = if let Some(index) = indexes.get(&row.id) {
            *index
        } else {
            let index = contacts.len();
            indexes.insert(row.id.clone(), index);
            contacts.push(Contact {
                id: row.id.clone(),
                full_name: row.full_name,
                title: row.title,
                organization: row.organization,
                email: row.email,
                phone: row.phone,
                address: row.address,
                website: row.website,
                types: row
                    .types
                    .iter()
                    .filter_map(|slug| ContactType::from_slug(slug))
                    .collect(),
                categories: Vec::new(),
                archived: row.archived,
                has_account: row.has_account,
                updated_at: stamp(row.updated_at),
                filtered_properties: Vec::new(),
                view_summary: None,
            });
            index
        };
        if let (Some(id), Some(name)) = (row.category_id, row.category_name) {
            contacts[index].categories.push(ContactCategory {
                id,
                name,
                parent_id: row.parent_id,
                parent_name: row.parent_name.unwrap_or_default(),
            });
        }
    }
    contacts
}

/// The organization owns its taxonomy: categories are seeded once by migration
/// 0035 and edited from there on. Nothing re-inserts them at boot, so a
/// category the organization deletes stays deleted.
pub async fn list_categories() -> Result<Vec<ContactCategory>, sqlx::Error> {
    sqlx::query_as::<_, CategoryRow>(
        "SELECT category.id, category.name, category.parent_id,
                COALESCE(parent.name, '') AS parent_name
         FROM contact_categories category
         LEFT JOIN contact_categories parent ON parent.id = category.parent_id
         ORDER BY COALESCE(parent.ord, category.ord), COALESCE(parent.name, category.name),
                  category.parent_id NULLS FIRST, category.ord, category.name",
    )
    .fetch_all(pool())
    .await
    .map(|rows| rows.into_iter().map(Into::into).collect())
}

/// Append a category at the end of its level, so a new row does not displace
/// the order the organization arranged.
pub async fn create_category(name: &str, parent_id: Option<&str>) -> Result<String, sqlx::Error> {
    let id = ids::opaque("cc");
    sqlx::query(
        "INSERT INTO contact_categories (id, name, parent_id, ord)
         VALUES ($1, $2, $3, COALESCE(
             (SELECT max(ord) + 1 FROM contact_categories
              WHERE parent_id IS NOT DISTINCT FROM $3),
             0
         ))",
    )
    .bind(&id)
    .bind(name)
    .bind(parent_id)
    .execute(pool())
    .await?;
    Ok(id)
}

/// Create many subcategories under one parent in a single transaction, for the
/// paste-a-list box. Names already present are skipped rather than failing the
/// whole paste, so pasting a corrected list twice is safe.
pub async fn create_subcategories(
    parent_id: &str,
    names: &[String],
) -> Result<usize, sqlx::Error> {
    let mut tx = pool().begin().await?;
    let mut next_ord: i32 = sqlx::query_scalar(
        "SELECT COALESCE(max(ord) + 1, 0) FROM contact_categories WHERE parent_id = $1",
    )
    .bind(parent_id)
    .fetch_one(&mut *tx)
    .await?;

    let mut added = 0usize;
    for name in names {
        let inserted = sqlx::query(
            "INSERT INTO contact_categories (id, name, parent_id, ord)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
        )
        .bind(ids::opaque("cc"))
        .bind(name)
        .bind(parent_id)
        .bind(next_ord)
        .execute(&mut *tx)
        .await?;
        if inserted.rows_affected() > 0 {
            next_ord += 1;
            added += 1;
        }
    }
    tx.commit().await?;
    Ok(added)
}

pub async fn rename_category(category_id: &str, name: &str) -> Result<(), sqlx::Error> {
    let result = sqlx::query("UPDATE contact_categories SET name = $2 WHERE id = $1")
        .bind(category_id)
        .bind(name)
        .execute(pool())
        .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

/// Store the given ids as the display order of their level.
pub async fn reorder_categories(category_ids: &[String]) -> Result<(), sqlx::Error> {
    let mut tx = pool().begin().await?;
    for (index, id) in category_ids.iter().enumerate() {
        sqlx::query("UPDATE contact_categories SET ord = $2 WHERE id = $1")
            .bind(id)
            .bind(index as i32)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// How many contacts and subcategories a category still holds, so the caller
/// can explain why a delete is refused rather than surfacing a foreign-key error.
pub async fn category_usage(category_id: &str) -> Result<(i64, i64), sqlx::Error> {
    let assignments: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM contact_category_assignments WHERE category_id = $1",
    )
    .bind(category_id)
    .fetch_one(pool())
    .await?;
    let children: i64 =
        sqlx::query_scalar("SELECT count(*) FROM contact_categories WHERE parent_id = $1")
            .bind(category_id)
            .fetch_one(pool())
            .await?;
    Ok((assignments, children))
}

/// Delete a category that nothing depends on. Rule options referencing it are
/// removed with it (`ON DELETE CASCADE`), so a rule never offers a category
/// that no longer exists.
pub async fn delete_category(category_id: &str) -> Result<(), sqlx::Error> {
    let result = sqlx::query("DELETE FROM contact_categories WHERE id = $1")
        .bind(category_id)
        .execute(pool())
        .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}

pub async fn category_is_root(category_id: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (
             SELECT 1 FROM contact_categories
             WHERE id = $1 AND parent_id IS NULL
         )",
    )
    .bind(category_id)
    .fetch_one(pool())
    .await
}

/// A bounded page of active directory contacts with an accurate result count.
pub async fn search_page(
    query: &str,
    category_ids: &[String],
    contact_type: Option<ContactType>,
    organization_id: &str,
    include_archived: bool,
    filters: &[PropertyFilter],
    offset: i64,
    limit: i64,
    include_account_projection: bool,
) -> Result<Page<Contact>, sqlx::Error> {
    let pattern = format!("%{query}%");
    let offset = offset.max(0);
    let limit = limit.clamp(1, 100);
    let (directory_columns, display_name_sql, account_search_sql) = if include_account_projection {
        (
            DIRECTORY_COLUMNS,
            DISPLAY_NAME_SQL,
            "OR u.first_name ILIKE $2 OR u.last_name ILIKE $2 OR u.email ILIKE $2",
        )
    } else {
        (SAFE_DIRECTORY_COLUMNS, SAFE_DISPLAY_NAME_SQL, "")
    };
    // A typed word also matches a property value, so "Boston" finds a contact
    // whose Location says so without building a filter first.
    let property_search_sql =
        property_filters::keyword_match_sql(PropertySubject::Contact, "c.id", 2);
    let property_filter_sql = property_filters::predicate_sql(PropertySubject::Contact, "c.id", 7);
    let filters_json = property_filters::to_json(filters);
    let filter = format!(
        "WHERE (
             $1 = '' OR c.first_name ILIKE $2 OR c.last_name ILIKE $2
             OR c.preferred_name ILIKE $2 {account_search_sql}
             OR {display_name_sql} ILIKE $2
             OR c.job_title ILIKE $2 OR o.name ILIKE $2
             OR c.email ILIKE $2 OR c.phone ILIKE $2
             OR c.mobile ILIKE $2 OR c.address ILIKE $2 OR c.website ILIKE $2
             {property_search_sql}
         )
         AND (
             cardinality($3::text[]) = 0 OR c.id IN (
                 SELECT selected.contact_id
                 FROM contact_category_assignments selected
                 WHERE selected.category_id = ANY($3::text[])
                 GROUP BY selected.contact_id
                 HAVING count(DISTINCT selected.category_id) = cardinality($3::text[])
             )
         )
         AND ($4 = '' OR $4 = ANY(c.types))
         AND ($5 = '' OR c.organization_id = $5)
         AND ($6 OR NOT c.archived)
         {property_filter_sql}"
    );
    let base = "FROM contacts c
                LEFT JOIN organizations o ON o.id = c.organization_id
                LEFT JOIN users u ON u.id = c.user_id";
    let count_sql = format!("SELECT count(*) {base} {filter}");
    let ids_sql = format!(
        "SELECT c.id {base} {filter}
         ORDER BY lower(c.last_name), lower(c.first_name), lower(coalesce(o.name, '')), c.id
         LIMIT $8 OFFSET $9"
    );

    let contact_type = contact_type.map(ContactType::slug).unwrap_or_default();
    let total_fut = sqlx::query_scalar::<_, i64>(&count_sql)
        .bind(query)
        .bind(&pattern)
        .bind(category_ids)
        .bind(contact_type)
        .bind(organization_id)
        .bind(include_archived)
        .bind(&filters_json)
        .fetch_one(pool());
    let ids_fut = sqlx::query_scalar::<_, String>(&ids_sql)
        .bind(query)
        .bind(&pattern)
        .bind(category_ids)
        .bind(contact_type)
        .bind(organization_id)
        .bind(include_archived)
        .bind(&filters_json)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool());
    let (total, ids) = tokio::try_join!(total_fut, ids_fut)?;
    if ids.is_empty() {
        return Ok(Page {
            items: Vec::new(),
            total,
        });
    }

    let rows = sqlx::query_as::<_, ContactRow>(&format!(
        "SELECT {directory_columns}
         {DIRECTORY_JOINS}
         WHERE c.id = ANY($1::text[])
         ORDER BY lower(c.last_name), lower(c.first_name), lower(coalesce(o.name, '')),
                  c.id, category.name"
    ))
    .bind(&ids)
    .fetch_all(pool())
    .await?;

    // The values behind the active chips, so each card can name why it matched.
    let filtered_keys: Vec<String> = filters.iter().map(|filter| filter.key.clone()).collect();
    let matched =
        property_filters::values_for(PropertySubject::Contact, &ids, &filtered_keys).await?;
    let mut items = fold_contacts(rows);
    for contact in &mut items {
        contact.filtered_properties = matched.get(&contact.id).cloned().unwrap_or_default();
    }
    // Case and application detail comes from the linked account, so it follows
    // the same admin-only rule as the account projection above.
    if include_account_projection {
        // `contact_type` is the bound slug by this point.
        let summaries = match ContactType::from_slug(contact_type) {
            Some(ContactType::Client) => client_summaries(&ids).await?,
            Some(ContactType::Volunteer) => volunteer_summaries(&ids).await?,
            _ => HashMap::new(),
        };
        for contact in &mut items {
            contact.view_summary = summaries.get(&contact.id).cloned();
        }
    }
    Ok(Page { items, total })
}

/// Case counts for client contacts: cases they are the client on, or own.
async fn client_summaries(
    ids: &[String],
) -> Result<HashMap<String, ContactViewSummary>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT ct.id,
                count(DISTINCT k.id) FILTER (WHERE k.status IN ('pending_review', 'open', 'monitor')),
                count(DISTINCT k.id) FILTER (WHERE k.status = 'closed')
         FROM contacts ct
         LEFT JOIN cases k
             ON k.id IN (SELECT cc.case_id FROM case_contacts cc
                         WHERE cc.contact_id = ct.id AND cc.role = 'client')
             OR (ct.user_id IS NOT NULL AND k.owner_id = ct.user_id)
         WHERE ct.id = ANY($1::text[])
         GROUP BY ct.id",
    )
    .bind(ids)
    .fetch_all(pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, active_cases, closed_cases)| {
            let summary = ContactViewSummary {
                active_cases,
                closed_cases,
                volunteer_status: String::new(),
            };
            (id, summary)
        })
        .collect())
}

/// Application status and active case count for volunteer contacts.
async fn volunteer_summaries(
    ids: &[String],
) -> Result<HashMap<String, ContactViewSummary>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, bool, Option<String>, i64)>(
        "SELECT ct.id, ct.user_id IS NOT NULL, v.status,
                (SELECT count(DISTINCT a.case_id)
                 FROM case_assignments a
                 JOIN cases k ON k.id = a.case_id
                 WHERE a.user_id = ct.user_id AND k.status IN ('open', 'monitor'))
         FROM contacts ct
         LEFT JOIN volunteers v ON v.user_id = ct.user_id
         WHERE ct.id = ANY($1::text[])",
    )
    .bind(ids)
    .fetch_all(pool())
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, has_account, status, active_cases)| {
            let volunteer_status = match (has_account, status.as_deref()) {
                (false, _) => "No account yet",
                (true, Some("approved")) => "Approved",
                (true, Some("pending")) => "Application pending",
                (true, Some("denied")) => "Application declined",
                (true, Some("revoked")) => "Volunteer role removed",
                (true, _) => "No volunteer application",
            };
            let summary = ContactViewSummary {
                active_cases,
                closed_cases: 0,
                volunteer_status: volunteer_status.to_string(),
            };
            (id, summary)
        })
        .collect())
}

pub async fn get(
    contact_id: &str,
    include_account_projection: bool,
) -> Result<Option<ContactDetails>, sqlx::Error> {
    let directory_columns = if include_account_projection {
        DIRECTORY_COLUMNS
    } else {
        SAFE_DIRECTORY_COLUMNS
    };
    let rows = sqlx::query_as::<_, ContactRow>(&format!(
        "SELECT {directory_columns}
         {DIRECTORY_JOINS}
         WHERE c.id = $1
         ORDER BY category.name"
    ))
    .bind(contact_id)
    .fetch_all(pool())
    .await?;
    let Some(contact) = fold_contacts(rows).into_iter().next() else {
        return Ok(None);
    };

    #[derive(sqlx::FromRow)]
    struct CommunicationRow {
        id: String,
        kind: String,
        body: String,
        author_name: String,
        occurred_at: chrono::DateTime<chrono::Utc>,
    }

    let communications = sqlx::query_as::<_, CommunicationRow>(
        "SELECT id, kind, body, author_name, occurred_at
         FROM contact_communications
         WHERE contact_id = $1
         ORDER BY occurred_at DESC, created_at DESC",
    )
    .bind(contact_id)
    .fetch_all(pool())
    .await?
    .into_iter()
    .filter_map(|row| {
        Some(ContactCommunication {
            id: row.id,
            kind: CommunicationKind::from_slug(&row.kind)?,
            body: row.body,
            author_name: row.author_name,
            occurred_at: stamp(row.occurred_at),
        })
    })
    .collect();
    Ok(Some(ContactDetails {
        contact,
        communications,
    }))
}

/// Split a directory-entered name into the stored first/last pair. A single word
/// becomes the last name, which is what `contacts_named_check` requires.
fn split_name(full_name: &str) -> (String, String) {
    let trimmed = full_name.trim();
    match trimmed.rsplit_once(char::is_whitespace) {
        Some((first, last)) => (first.trim().to_string(), last.trim().to_string()),
        None => (String::new(), trimmed.to_string()),
    }
}

/// Map the directory's free-text employer onto an organization record, creating
/// one when the name is new. Archived organizations are reused only if nothing
/// active matches, which would fail validation later with a readable message.
async fn resolve_organization(
    name: &str,
    actor_user_id: &str,
    actor: &str,
) -> Result<String, sqlx::Error> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(String::new());
    }
    if let Some(id) = sqlx::query_scalar::<_, String>(
        "SELECT id FROM organizations
         WHERE lower(btrim(name)) = lower(btrim($1)) AND NOT archived
         LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool())
    .await?
    {
        return Ok(id);
    }
    organizations::create(
        &OrganizationInput {
            name: name.to_string(),
            kind: OrganizationKind::Other,
            ..Default::default()
        },
        actor_user_id,
        actor,
    )
    .await
}

/// Reject a selection that breaks a rule's maximum, and return the rules it
/// fires so the caller can apply their fields.
///
/// Only the maximum is enforced. A shortfall against `min_choices` is advisory:
/// blocking it would strand imported or part-filled records as un-editable.
async fn check_rules(
    type_slugs: &[String],
    category_ids: &[String],
) -> Result<Vec<crate::server_fns::contact_rules::ContactRule>, sqlx::Error> {
    let rules = contact_rules::matching(type_slugs, category_ids).await?;
    let problems = contact_rules::over_limit(&rules, category_ids);
    if !problems.is_empty() {
        return Err(sqlx::Error::Protocol(problems.join(" ")));
    }
    Ok(rules)
}

/// Create or update a person from the directory form, then rewrite their
/// classifications. Identity fields are written through
/// [`crate::server::db::contacts`]; the fields the directory does not collect
/// keep whatever Admin last set.
pub async fn save(
    contact_id: Option<&str>,
    input: &ContactInput,
    actor_user_id: &str,
    actor: &str,
) -> Result<String, sqlx::Error> {
    let known_categories: i64 =
        sqlx::query_scalar("SELECT count(*) FROM contact_categories WHERE id = ANY($1::text[])")
            .bind(&input.category_ids)
            .fetch_one(pool())
            .await?;
    if known_categories != input.category_ids.len() as i64 {
        return Err(sqlx::Error::Protocol(
            "One or more selected contact categories no longer exist.".to_string(),
        ));
    }

    let type_slugs: Vec<String> = input
        .types
        .iter()
        .map(|value| value.slug().to_string())
        .collect();
    let rules = check_rules(&type_slugs, &input.category_ids).await?;

    let (first_name, last_name) = split_name(&input.full_name);
    // Validate canonical contact fields before organization resolution can write.
    PersonInput {
        first_name: first_name.clone(),
        last_name: last_name.clone(),
        email: input.email.clone(),
        phone: input.phone.clone(),
        address: input.address.clone(),
        job_title: input.title.clone(),
        types: input.types.clone(),
        ..Default::default()
    }
    .validate()
    .map_err(sqlx::Error::Protocol)?;

    let id = match contact_id {
        Some(id) => {
            let existing = contacts::get(id, true)
                .await?
                .ok_or(sqlx::Error::RowNotFound)?;
            // A linked person's name, email, phone and address are owned by their
            // account, so editing them here would be silently discarded.
            if existing.has_account() {
                return Err(sqlx::Error::Protocol(
                    "This person signs in with an account; edit their identity from the authoritative account/profile path.".into(),
                ));
            }
            let organization_id =
                resolve_organization(&input.organization, actor_user_id, actor).await?;
            let person = PersonInput {
                first_name,
                last_name,
                email: input.email.clone(),
                phone: input.phone.clone(),
                address: input.address.clone(),
                job_title: input.title.clone(),
                organization_id,
                preferred_name: existing.preferred_name,
                mobile: existing.mobile,
                types: input.types.clone(),
                source: existing.source,
                description: existing.description,
                do_not_contact: existing.do_not_contact,
            };
            let person = person.validate().map_err(sqlx::Error::Protocol)?;
            contacts::update(id, &person, actor_user_id, actor).await?;
            id.to_string()
        }
        None => {
            let organization_id =
                resolve_organization(&input.organization, actor_user_id, actor).await?;
            let person = PersonInput {
                first_name,
                last_name,
                email: input.email.clone(),
                phone: input.phone.clone(),
                address: input.address.clone(),
                job_title: input.title.clone(),
                organization_id,
                types: input.types.clone(),
                source: DIRECTORY_SOURCE.to_string(),
                ..Default::default()
            };
            let person = person.validate().map_err(sqlx::Error::Protocol)?;
            contacts::create(&person, actor_user_id, actor).await?
        }
    };

    let mut tx = pool().begin().await?;
    sqlx::query("UPDATE contacts SET website = $2, updated_at = now() WHERE id = $1")
        .bind(&id)
        .bind(&input.website)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM contact_category_assignments WHERE contact_id = $1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    // Assign each selected classification and its parent. A contact tagged
    // "Family law" is therefore also found by the broader "Legal" filter.
    sqlx::query(
        "INSERT INTO contact_category_assignments (contact_id, category_id)
         SELECT $1, category_id
         FROM (
             SELECT id AS category_id
             FROM contact_categories
             WHERE id = ANY($2::text[])
             UNION
             SELECT parent_id
             FROM contact_categories
             WHERE id = ANY($2::text[]) AND parent_id IS NOT NULL
         ) selected",
    )
    .bind(&id)
    .bind(&input.category_ids)
    .execute(&mut *tx)
    .await?;

    // Blank out the property fields the matching rules suggest, so a vendor
    // arrives with the vendor questions already waiting to be answered.
    contact_rules::apply_fields_in_transaction(&mut tx, &id, &rules).await?;

    tx.commit().await?;
    Ok(id)
}

/// Replace category membership without rewriting the contact's identity fields.
pub async fn set_categories(
    contact_id: &str,
    category_ids: &[String],
    actor_user_id: &str,
    actor: &str,
) -> Result<(), sqlx::Error> {
    let mut selected = category_ids.to_vec();
    selected.sort();
    selected.dedup();

    // Rules are keyed on the contact's own types plus the incoming selection.
    let type_slugs: Vec<String> =
        sqlx::query_scalar("SELECT unnest(types) FROM contacts WHERE id = $1")
            .bind(contact_id)
            .fetch_all(pool())
            .await?;
    let rules = check_rules(&type_slugs, &selected).await?;

    let mut tx = pool().begin().await?;
    let exists: Option<i32> = sqlx::query_scalar("SELECT 1 FROM contacts WHERE id = $1 FOR UPDATE")
        .bind(contact_id)
        .fetch_optional(&mut *tx)
        .await?;
    if exists.is_none() {
        return Err(sqlx::Error::RowNotFound);
    }

    let known: i64 =
        sqlx::query_scalar("SELECT count(*) FROM contact_categories WHERE id = ANY($1::text[])")
            .bind(&selected)
            .fetch_one(&mut *tx)
            .await?;
    if known != selected.len() as i64 {
        return Err(sqlx::Error::Protocol(
            "One or more selected contact categories no longer exist.".to_string(),
        ));
    }

    let mut expanded: Vec<String> = sqlx::query_scalar(
        "SELECT category_id
         FROM (
             SELECT id AS category_id
             FROM contact_categories
             WHERE id = ANY($1::text[])
             UNION
             SELECT parent_id
             FROM contact_categories
             WHERE id = ANY($1::text[]) AND parent_id IS NOT NULL
         ) selected
         ORDER BY category_id",
    )
    .bind(&selected)
    .fetch_all(&mut *tx)
    .await?;
    expanded.sort();

    let mut current: Vec<String> = sqlx::query_scalar(
        "SELECT category_id FROM contact_category_assignments
         WHERE contact_id = $1 ORDER BY category_id",
    )
    .bind(contact_id)
    .fetch_all(&mut *tx)
    .await?;
    current.sort();

    // Applied before the no-op check: a rule added since the last save should
    // still bring its fields, even when the categories themselves did not move.
    contact_rules::apply_fields_in_transaction(&mut tx, contact_id, &rules).await?;

    if current == expanded {
        tx.commit().await?;
        return Ok(());
    }

    sqlx::query("DELETE FROM contact_category_assignments WHERE contact_id = $1")
        .bind(contact_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO contact_category_assignments (contact_id, category_id)
         SELECT $1, category_id
         FROM (
             SELECT id AS category_id
             FROM contact_categories
             WHERE id = ANY($2::text[])
             UNION
             SELECT parent_id
             FROM contact_categories
             WHERE id = ANY($2::text[]) AND parent_id IS NOT NULL
         ) selected",
    )
    .bind(contact_id)
    .bind(&selected)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE contacts SET updated_at = now() WHERE id = $1")
        .bind(contact_id)
        .execute(&mut *tx)
        .await?;
    audit::record_in_transaction_by(
        &mut tx,
        audit::Entity::Contact,
        contact_id,
        actor_user_id,
        actor,
        "categories",
        &current.join(", "),
        &expanded.join(", "),
    )
    .await?;
    tx.commit().await
}

pub async fn add_communication(
    contact_id: &str,
    kind: CommunicationKind,
    body: &str,
    author_id: &str,
    author_name: &str,
) -> Result<(), sqlx::Error> {
    let id = ids::opaque("comm");
    let result = sqlx::query(
        "INSERT INTO contact_communications
             (id, contact_id, kind, body, author_id, author_name)
         SELECT $1, id, $3, $4, $5, $6 FROM contacts WHERE id = $2",
    )
    .bind(id)
    .bind(contact_id)
    .bind(kind.slug())
    .bind(body)
    .bind(author_id)
    .bind(author_name)
    .execute(pool())
    .await?;
    if result.rows_affected() == 0 {
        return Err(sqlx::Error::RowNotFound);
    }
    Ok(())
}
