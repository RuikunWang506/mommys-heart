//! Queries behind [`crate::server_fns::duplicates`]: groups of cases with the
//! same name, and groups of contacts that share a name, email, or phone number.
//! Read-only; nothing here changes a record.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::server::db::pool;
use crate::server_fns::cases::CaseStatus;
use crate::server_fns::contacts::ContactType;
use crate::server_fns::duplicates::{
    DuplicateCase, DuplicateCaseGroup, DuplicateContact, DuplicateContactGroup, MatchReason,
};

/// A contact's identity columns, read from the linked account when there is one
/// (`ct` is the contact row, `u` the optional account).
const FIRST: &str = "COALESCE(CASE WHEN u.id IS NULL THEN ct.first_name ELSE u.first_name END, '')";
const LAST: &str = "COALESCE(CASE WHEN u.id IS NULL THEN ct.last_name ELSE u.last_name END, '')";
const EMAIL: &str = "COALESCE(CASE WHEN u.id IS NULL THEN ct.email ELSE u.email END, '')";
const PHONE: &str = "COALESCE(CASE WHEN u.id IS NULL THEN ct.phone ELSE u.phone END, '')";

/// SQL that turns a name into a comparison key: trimmed, lower-case, and with
/// runs of whitespace collapsed, so "Silva  Appeal " matches "silva appeal".
fn name_key(column: &str) -> String {
    format!("lower(regexp_replace(btrim({column}), '\\s+', ' ', 'g'))")
}

/// Groups of two or more cases whose names match.
pub async fn case_groups() -> Result<Vec<DuplicateCaseGroup>, sqlx::Error> {
    let key = name_key("c.name");
    let sql = format!(
        "WITH keyed AS (
             SELECT c.id, {key} AS k FROM cases c WHERE btrim(c.name) <> ''
         ),
         dup AS (
             SELECT k FROM keyed GROUP BY k HAVING count(*) > 1
         )
         SELECT keyed.k, c.id, c.name, c.status,
                btrim(concat_ws(' ', u.first_name, u.last_name)) AS filed_by,
                COALESCE((
                    SELECT array_agg(btrim(concat_ws(' ', ct.first_name, ct.last_name))
                                     ORDER BY ct.last_name, ct.first_name)
                           FILTER (WHERE btrim(concat_ws(' ', ct.first_name, ct.last_name)) <> '')
                    FROM case_contacts cc
                    JOIN contacts ct ON ct.id = cc.contact_id
                    WHERE cc.case_id = c.id AND cc.role = 'client'
                ), '{{}}'::text[]) AS clients,
                (SELECT count(*) FROM case_notes n
                  WHERE n.case_id = c.id AND n.state <> 'discarded') AS note_count,
                (SELECT count(*) FROM messages m WHERE m.case_id = c.id) AS message_count,
                (SELECT max(m.sent_at) FROM messages m WHERE m.case_id = c.id) AS last_message
         FROM keyed
         JOIN dup USING (k)
         JOIN cases c ON c.id = keyed.id
         LEFT JOIN users u ON u.id = c.owner_id
         ORDER BY keyed.k, length(c.id), c.id"
    );

    type Row = (
        String,
        String,
        String,
        String,
        Option<String>,
        Vec<String>,
        i64,
        i64,
        Option<String>,
    );
    let rows: Vec<Row> = sqlx::query_as(&sql).fetch_all(pool()).await?;

    let mut groups: Vec<DuplicateCaseGroup> = Vec::new();
    let mut current_key: Option<String> = None;
    for (k, id, name, status, filed_by, clients, note_count, message_count, last_message) in rows {
        if current_key.as_deref() != Some(k.as_str()) {
            groups.push(DuplicateCaseGroup {
                cases: Vec::new(),
                dismissed: false,
            });
            current_key = Some(k);
        }
        let filed_by = filed_by.filter(|n| !n.is_empty()).unwrap_or_default();
        if let Some(group) = groups.last_mut() {
            group.cases.push(DuplicateCase {
                id,
                name,
                status: CaseStatus::from_slug(&status).unwrap_or(CaseStatus::Open),
                filed_by,
                clients,
                note_count,
                message_count,
                last_message,
            });
        }
    }

    Ok(groups)
}

/// Groups of two or more active contacts connected by a shared name, email
/// address, or phone number (any of a contact's phone or mobile, compared by
/// their last ten digits). Contacts linked through a chain of matches land in
/// one group, with every reason that linked them.
pub async fn contact_groups() -> Result<Vec<DuplicateContactGroup>, sqlx::Error> {
    // A contact linked to an account takes its name, email and phone from the
    // account, as the contact directory shows it, so compare those values.
    let key = name_key(&format!("concat_ws(' ', {FIRST}, {LAST})"));
    let sql = format!(
        "WITH base AS (
             SELECT ct.id, {LAST} AS last_name,
                    {key} AS name_key,
                    lower(btrim({EMAIL})) AS email_key,
                    right(regexp_replace({PHONE}, '\\D', '', 'g'), 10) AS phone_key,
                    right(regexp_replace(ct.mobile, '\\D', '', 'g'), 10) AS mobile_key
             FROM contacts ct
             LEFT JOIN users u ON u.id = ct.user_id
             WHERE NOT ct.archived
         ),
         keys AS (
             SELECT id, 'name' AS kind, name_key AS k FROM base WHERE btrim(last_name) <> ''
             UNION SELECT id, 'email', email_key FROM base WHERE email_key <> ''
             UNION SELECT id, 'phone', phone_key FROM base WHERE length(phone_key) >= 7
             UNION SELECT id, 'phone', mobile_key FROM base WHERE length(mobile_key) >= 7
         ),
         dup AS (
             SELECT kind, k FROM keys GROUP BY kind, k HAVING count(DISTINCT id) > 1
         )
         SELECT keys.kind, keys.k, keys.id FROM keys JOIN dup USING (kind, k)
         ORDER BY keys.kind, keys.k, keys.id"
    );
    let matches: Vec<(String, String, String)> = sqlx::query_as(&sql).fetch_all(pool()).await?;
    if matches.is_empty() {
        return Ok(Vec::new());
    }

    // Union-find over contact ids, so A~B (same email) and B~C (same phone)
    // become one group rather than two overlapping ones.
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut ids: Vec<String> = Vec::new();
    for (_, _, id) in &matches {
        if !index.contains_key(id) {
            index.insert(id.clone(), ids.len());
            ids.push(id.clone());
        }
    }
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }

    // Each (kind, key) bucket, with the reason it stands for.
    let mut buckets: Vec<(MatchReason, Vec<usize>)> = Vec::new();
    let mut last: Option<(String, String)> = None;
    for (kind, k, id) in &matches {
        let reason = match kind.as_str() {
            "email" => MatchReason::Email,
            "phone" => MatchReason::Phone,
            _ => MatchReason::Name,
        };
        if last.as_ref() != Some(&(kind.clone(), k.clone())) {
            buckets.push((reason, Vec::new()));
            last = Some((kind.clone(), k.clone()));
        }
        if let Some((_, members)) = buckets.last_mut() {
            members.push(index[id]);
        }
    }
    for (_, members) in &buckets {
        for pair in members.windows(2) {
            let a = find(&mut parent, pair[0]);
            let b = find(&mut parent, pair[1]);
            if a != b {
                parent[b] = a;
            }
        }
    }

    let mut components: HashMap<usize, (BTreeSet<MatchReason>, Vec<usize>)> = HashMap::new();
    for (reason, members) in &buckets {
        let root = find(&mut parent, members[0]);
        components.entry(root).or_default().0.insert(*reason);
    }
    for i in 0..ids.len() {
        let root = find(&mut parent, i);
        components.entry(root).or_default().1.push(i);
    }

    let details = contact_details(&ids).await?;

    let mut groups: Vec<DuplicateContactGroup> = components
        .into_values()
        .map(|(reasons, members)| {
            let mut contacts: Vec<DuplicateContact> = members
                .into_iter()
                .filter_map(|i| details.get(&ids[i]).cloned())
                .collect();
            contacts.sort_by(|a, b| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then(a.id.cmp(&b.id))
            });
            DuplicateContactGroup {
                reasons: reasons.into_iter().collect(),
                contacts,
                dismissed: false,
            }
        })
        .filter(|group| group.contacts.len() > 1)
        .collect();
    groups.sort_by(|a, b| {
        let first = |g: &DuplicateContactGroup| {
            g.contacts
                .first()
                .map(|c| c.name.to_lowercase())
                .unwrap_or_default()
        };
        first(a).cmp(&first(b))
    });

    Ok(groups)
}

/// Display details for the given contacts, keyed by id.
async fn contact_details(ids: &[String]) -> Result<HashMap<String, DuplicateContact>, sqlx::Error> {
    type Row = (
        String,
        String,
        String,
        String,
        String,
        String,
        Vec<String>,
        bool,
        String,
        i64,
    );
    let rows: Vec<Row> = sqlx::query_as(
        &format!(
            "SELECT ct.id, {FIRST}, {LAST}, {EMAIL}, {PHONE}, ct.mobile, ct.types,
                    u.id IS NOT NULL AS has_account,
                    to_char(ct.updated_at, 'YYYY-MM-DD') AS updated_on,
                    (SELECT count(*) FROM case_contacts cc WHERE cc.contact_id = ct.id) AS case_count
             FROM contacts ct
             LEFT JOIN users u ON u.id = ct.user_id
             WHERE ct.id = ANY($1)"
        ),
    )
    .bind(ids)
    .fetch_all(pool())
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(
                id,
                first,
                last,
                email,
                phone,
                mobile,
                types,
                has_account,
                updated_on,
                case_count,
            )| {
                let name = format!("{} {}", first.trim(), last.trim())
                    .trim()
                    .to_string();
                let phone = if phone.trim().is_empty() {
                    mobile
                } else {
                    phone
                };
                let contact = DuplicateContact {
                    id: id.clone(),
                    name,
                    email,
                    phone,
                    types: types
                        .iter()
                        .filter_map(|t| ContactType::from_slug(t))
                        .collect(),
                    case_count,
                    has_account,
                    updated_on,
                };
                (id, contact)
            },
        )
        .collect())
}

/// The id sets marked "not duplicates" for one kind (`"case"` or `"contact"`),
/// each sorted.
pub async fn dismissed_sets(kind: &str) -> Result<HashSet<Vec<String>>, sqlx::Error> {
    let rows: Vec<(Vec<String>,)> =
        sqlx::query_as("SELECT record_ids FROM duplicate_dismissals WHERE kind = $1")
            .bind(kind)
            .fetch_all(pool())
            .await?;
    Ok(rows.into_iter().map(|(ids,)| ids).collect())
}

/// Mark a group as not duplicates. `ids` must already be sorted and unique.
pub async fn dismiss(
    kind: &str,
    ids: &[String],
    actor_id: &str,
    actor_name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO duplicate_dismissals (kind, record_ids, dismissed_by_id, dismissed_by_name)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (kind, record_ids) DO NOTHING",
    )
    .bind(kind)
    .bind(ids)
    .bind(actor_id)
    .bind(actor_name)
    .execute(pool())
    .await?;
    Ok(())
}

/// Undo [`dismiss`], so the group is listed again.
pub async fn undismiss(kind: &str, ids: &[String]) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM duplicate_dismissals WHERE kind = $1 AND record_ids = $2")
        .bind(kind)
        .bind(ids)
        .execute(pool())
        .await?;
    Ok(())
}
