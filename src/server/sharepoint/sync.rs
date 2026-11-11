//! Provisioning a case's folder, and keeping the library's sharing invitations
//! in step with the app's own permissions.
//!
//! Two operations live here, and between them they are the only things that
//! write to the library's *structure* or its *access*:
//!
//! * [`ensure_case_folder`] — create a case's folder and its standing
//!   sub-folders, idempotently.
//! * [`sync_case_access`] — make the invitations on those folders match the
//!   capabilities recorded in `case_assignments`.
//!
//! # Why reconcile rather than grant and revoke in place
//!
//! Case access changes in six different places (assignment, a single capability
//! toggle, a batch save, unassignment, an approved access request, account
//! deactivation). Issuing an invitation at each of them would mean six chances
//! to forget the matching revoke. Instead every one of those places calls
//! [`sync_case_access`], which computes what access *should* look like and
//! changes only the difference. Granting and revoking are then the same code
//! path, and a call that fails is repaired by the next one rather than leaving
//! access permanently wrong.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use tokio::sync::Mutex;

use crate::helpers::new_case_folders::NEW_CASE_FOLDERS;
use crate::server::db::{case_documents as db, cases};
use crate::server::sharepoint::{case_folder_name, store, DocumentRole};

/// Create a case's folder and the standing tree inside it, and record where it
/// landed. Returns the folder reference.
///
/// Idempotent in both directions: a folder that already exists is adopted rather
/// than duplicated, so this is safe to call again after a failure, on a case
/// created before the feature existed, or from the "Set up documents folder"
/// button on the case page.
pub async fn ensure_case_folder(case_id: &str) -> Result<db::CaseFolderRef, String> {
    // The same per-case lock the reconcile takes, so two requests arriving
    // together for an unprovisioned case cannot both walk the creation path.
    let case_lock = lock_for(case_id).await;
    let _guard = case_lock.lock().await;

    let store = store()?;

    let case_name = cases::name(case_id)
        .await
        .map_err(|e| format!("database error: {e}"))?
        .ok_or_else(|| "that case no longer exists".to_string())?;

    // An existing folder is reused: the id is what identifies it, so a case
    // renamed since provisioning keeps the folder it already has.
    let existing = db::folder_ref(case_id)
        .await
        .map_err(|e| format!("database error: {e}"))?;
    let case_folder = if existing.is_ready() {
        crate::server::sharepoint::FolderRef {
            item_id: existing.item_id.clone(),
            web_url: existing.web_url.clone(),
        }
    } else {
        let root = store.ensure_root().await?;
        let folder = store
            .ensure_folder(&root.item_id, &case_folder_name(case_id, &case_name))
            .await?;
        db::set_folder_ref(case_id, &folder.item_id, &folder.web_url)
            .await
            .map_err(|e| format!("database error: {e}"))?;
        folder
    };

    // The standing tree every case starts with. Declared in code, created here,
    // and never mirrored into a table.
    for spec in NEW_CASE_FOLDERS {
        let top = store.ensure_folder(&case_folder.item_id, spec.name).await?;
        // Children are paths listed parents first, so each parent's id is known
        // by the time its children are reached.
        let mut ids = std::collections::HashMap::new();
        for child in spec.children {
            let (parent, name) = match child.rsplit_once('/') {
                Some((parent, name)) => (ids.get(parent).cloned().unwrap_or_default(), name),
                None => (top.item_id.clone(), *child),
            };
            if parent.is_empty() {
                return Err(format!("folder \"{child}\" is listed before its parent"));
            }
            let folder = store.ensure_folder(&parent, name).await?;
            ids.insert(child.to_string(), folder.item_id);
        }
    }

    Ok(db::CaseFolderRef {
        item_id: case_folder.item_id,
        web_url: case_folder.web_url,
    })
}

/// The case's folder, creating it if the case does not have one yet.
///
/// This is what every document request resolves through, so a case that predates
/// the SharePoint feature — or whose provisioning failed when it was created —
/// gets its standing folders the first time somebody opens its Documents panel,
/// rather than showing an error nobody can act on.
///
/// It also recovers a case whose folder exists in the library but whose stored
/// pointer was lost (a database restored from an older backup, say):
/// [`ensure_case_folder`] adopts the folder that is already there by name rather
/// than creating a second one.
pub async fn ensure_folder_ref(case_id: &str) -> Result<db::CaseFolderRef, String> {
    let existing = db::folder_ref(case_id)
        .await
        .map_err(|e| format!("database error: {e}"))?;
    if existing.is_ready() {
        return Ok(existing);
    }

    tracing::info!("case {case_id} has no documents folder; creating it now");
    let folder = ensure_case_folder(case_id).await?;
    // The new folders need their sharing invitations. Spawned, so opening the
    // panel does not wait on it.
    sync_case_access(case_id.to_string());
    Ok(folder)
}

/// Make the library's sharing invitations for a case match the app's own
/// permissions. Best-effort: errors are logged, never returned to the caller.
///
/// Spawned rather than awaited by its callers, exactly like
/// [`crate::server::notifications::notify_case`], so a slow or unreachable
/// library never delays a permission change in the CRM.
pub fn sync_case_access(case_id: String) {
    tokio::spawn(async move {
        if let Err(e) = reconcile(&case_id).await {
            tracing::warn!("could not sync document access for case {case_id}: {e}");
        }
    });
}

/// One lock per case, so two reconciles of the same case never interleave.
///
/// Without this, two overlapping runs both read "nothing granted yet" and both
/// issue an invitation; the second overwrites the first's row and the first
/// permission is left live in the library with its id recorded nowhere — access
/// that can never be revoked. Several call sites fire at once (a batch save
/// spawns one task per case, two admins can edit the same case), so the overlap
/// is ordinary rather than exotic.
static CASE_LOCKS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();

async fn lock_for(case_id: &str) -> Arc<Mutex<()>> {
    let locks = CASE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = locks.lock().await;
    map.entry(case_id.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// The reconciliation itself, awaited. Separate from [`sync_case_access`] so the
/// one-shot check command and the startup reconciler can wait for the outcome
/// and report it.
pub async fn reconcile(case_id: &str) -> Result<(), String> {
    let case_lock = lock_for(case_id).await;
    let _guard = case_lock.lock().await;

    let store = store()?;

    let folder = db::folder_ref(case_id)
        .await
        .map_err(|e| format!("database error: {e}"))?;
    if !folder.is_ready() {
        // Nothing to grant access *to* yet. Provisioning will call back here.
        return Ok(());
    }

    let desired = db::desired_grants(case_id)
        .await
        .map_err(|e| format!("database error: {e}"))?;
    let recorded = db::recorded_grants(case_id)
        .await
        .map_err(|e| format!("database error: {e}"))?;

    // What access *should* exist: one entry per (user, top-level folder) pair
    // the user's audience allows.
    let mut wanted: Vec<(String, String, DocumentRole, String)> = Vec::new();
    for grant in &desired {
        for spec in NEW_CASE_FOLDERS {
            // The audience gate. A client is never invited to a volunteer-only
            // folder, whatever capabilities they hold on the case.
            if spec.visibility.is_restricted() && !grant.sees_volunteer_only {
                continue;
            }
            wanted.push((
                grant.user_id.clone(),
                spec.name.to_string(),
                grant.role,
                grant.email.clone(),
            ));
        }
    }

    // Withdraw anything recorded that is no longer wanted, or whose role or
    // address has changed (re-granted below with the new one).
    //
    // One failure must not abort the pass: a revoke that cannot be completed now
    // would otherwise skip every remaining revoke *and* every grant, and would
    // keep doing so on each later attempt — turning one stuck permission into a
    // case whose access never updates again.
    for existing in &recorded {
        let still_wanted = wanted.iter().any(|(user_id, folder_name, role, email)| {
            *user_id == existing.user_id
                && *folder_name == existing.folder_name
                && role.slug() == existing.role
                && email.eq_ignore_ascii_case(&existing.email)
        });
        if still_wanted {
            continue;
        }
        let item_id = match top_folder_id(store, &folder.item_id, &existing.folder_name).await {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    "could not find folder '{}' on case {case_id} to revoke access: {e}",
                    existing.folder_name
                );
                continue;
            }
        };
        if let Err(e) = store.revoke(&item_id, &existing.permission_id).await {
            tracing::warn!(
                "could not revoke document access for {} on case {case_id} folder '{}': {e}",
                existing.user_id,
                existing.folder_name
            );
            continue;
        }
        db::forget_grant(case_id, &existing.user_id, &existing.folder_name)
            .await
            .map_err(|e| format!("database error: {e}"))?;
        tracing::info!(
            "revoked document access for {} on case {case_id} folder '{}'",
            existing.user_id,
            existing.folder_name
        );
    }

    // Issue anything wanted that is not already recorded exactly as wanted.
    for (user_id, folder_name, role, email) in &wanted {
        let already = recorded.iter().any(|existing| {
            existing.user_id == *user_id
                && existing.folder_name == *folder_name
                && existing.role == role.slug()
                && existing.email.eq_ignore_ascii_case(email)
        });
        if already {
            continue;
        }
        let item_id = match top_folder_id(store, &folder.item_id, folder_name).await {
            Ok(Some(id)) => id,
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!(
                    "could not find folder '{folder_name}' on case {case_id} to grant access: {e}"
                );
                continue;
            }
        };
        match store.invite(&item_id, email, *role).await {
            Ok(permission) => {
                db::record_grant(
                    case_id,
                    user_id,
                    folder_name,
                    &permission.permission_id,
                    *role,
                    email,
                )
                .await
                .map_err(|e| format!("database error: {e}"))?;
                tracing::info!(
                    "granted {} document access to {email} on case {case_id} folder '{folder_name}'",
                    role.slug()
                );
            }
            // One address the tenant will not share with (commonly an external
            // recipient where external sharing is off) must not stop the rest.
            Err(e) => tracing::warn!(
                "could not invite {email} to case {case_id} folder '{folder_name}': {e}"
            ),
        }
    }

    Ok(())
}

/// The library id of one of a case's top-level folders, or `None` when it is
/// not there — which happens only if provisioning was interrupted.
async fn top_folder_id(
    store: &dyn super::DocumentStore,
    case_item_id: &str,
    folder_name: &str,
) -> Result<Option<String>, String> {
    store.resolve_path(case_item_id, folder_name).await
}

/// Finish provisioning for cases that have no documents folder yet.
///
/// Runs once in the background at startup. A case created while the library was
/// unreachable gets its folder here rather than waiting for somebody to notice
/// and press a button.
pub fn start_provisioning_backfill() {
    tokio::spawn(async move {
        // Wait a moment so startup logging is not interleaved with this.
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let case_ids = match db::unprovisioned_case_ids(50).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("could not look for unprovisioned case folders: {e}");
                return;
            }
        };
        if case_ids.is_empty() {
            return;
        }
        tracing::info!(
            "creating document folders for {} case(s) that had none",
            case_ids.len()
        );
        for case_id in case_ids {
            match ensure_case_folder(&case_id).await {
                Ok(_) => {
                    if let Err(e) = reconcile(&case_id).await {
                        tracing::warn!("could not sync access for case {case_id}: {e}");
                    }
                }
                Err(e) => {
                    tracing::warn!("could not create the document folder for {case_id}: {e}");
                    // The library is likely unreachable; stop rather than
                    // hammering it once per case.
                    return;
                }
            }
        }
    });
}
