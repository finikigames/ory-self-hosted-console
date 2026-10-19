//! CRUD for the contours stored on the logged-in admin, plus the session switch.

use salvo::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::models::Session;
use crate::error::AppError;

use super::queries::{self, EnvironmentRow};
use super::{normalize_admin_base_url, normalize_keto, normalize_name, pool, MAX_ENVIRONMENTS};

#[derive(Debug, Serialize)]
struct EnvironmentView {
    id: Uuid,
    name: String,
    kratos_admin_url: String,
    hydra_admin_url: String,
    hydra_public_url: String,
    keto_read_url: Option<String>,
    keto_write_url: Option<String>,
    keto_opl_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct EnvironmentList {
    active_id: Option<Uuid>,
    environments: Vec<EnvironmentView>,
}

#[derive(Debug, Deserialize)]
struct EnvironmentBody {
    name: String,
    kratos_admin_url: String,
    hydra_admin_url: String,
    hydra_public_url: String,
    #[serde(default)]
    keto_read_url: Option<String>,
    #[serde(default)]
    keto_write_url: Option<String>,
    #[serde(default)]
    keto_opl_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ActiveBody {
    id: Option<Uuid>,
}

struct Normalized {
    name: String,
    kratos_admin_url: String,
    hydra_admin_url: String,
    hydra_public_url: String,
    keto_read_url: Option<String>,
    keto_write_url: Option<String>,
    keto_opl_url: Option<String>,
}

fn view(row: EnvironmentRow) -> EnvironmentView {
    EnvironmentView {
        id: row.id,
        name: row.name,
        kratos_admin_url: row.kratos_admin_url,
        hydra_admin_url: row.hydra_admin_url,
        hydra_public_url: row.hydra_public_url,
        keto_read_url: row.keto_read_url,
        keto_write_url: row.keto_write_url,
        keto_opl_url: row.keto_opl_url,
    }
}

fn session_ids(depot: &mut Depot) -> Result<(Uuid, Uuid), AppError> {
    let session = depot
        .obtain::<Session>()
        .map_err(|_| AppError::Unauthorized)?;
    Ok((session.id, session.admin_id))
}

fn path_id(req: &mut Request) -> Result<Uuid, AppError> {
    req.param::<String>("id")
        .and_then(|s| Uuid::parse_str(&s).ok())
        .ok_or(AppError::NotFound)
}

fn normalize_body(body: EnvironmentBody) -> Result<Normalized, AppError> {
    let name = normalize_name(&body.name)?;
    let kratos_admin_url = normalize_admin_base_url(&body.kratos_admin_url)?;
    let hydra_admin_url = normalize_admin_base_url(&body.hydra_admin_url)?;
    let hydra_public_url = normalize_admin_base_url(&body.hydra_public_url)?;
    let keto = normalize_keto(
        body.keto_read_url.as_deref(),
        body.keto_write_url.as_deref(),
        body.keto_opl_url.as_deref(),
    )?;
    let (keto_read_url, keto_write_url, keto_opl_url) = match keto {
        Some((read, write, opl)) => (Some(read), Some(write), Some(opl)),
        None => (None, None, None),
    };
    Ok(Normalized {
        name,
        kratos_admin_url,
        hydra_admin_url,
        hydra_public_url,
        keto_read_url,
        keto_write_url,
        keto_opl_url,
    })
}

/// `GET /api/console/environments` — this admin's contours and the active id.
#[handler]
pub async fn list_environments(depot: &mut Depot) -> Result<Json<EnvironmentList>, AppError> {
    let pool = pool(depot)?;
    let (session_id, admin_id) = session_ids(depot)?;
    let environments = queries::list_for_admin(&pool, admin_id)
        .await?
        .into_iter()
        .map(view)
        .collect();
    let active_id = queries::session_environment_id(&pool, session_id).await?;
    Ok(Json(EnvironmentList {
        active_id,
        environments,
    }))
}

/// `POST /api/console/environments` — add a contour on this admin account.
#[handler]
pub async fn create_environment(
    req: &mut Request,
    depot: &mut Depot,
) -> Result<Json<EnvironmentView>, AppError> {
    let pool = pool(depot)?;
    let (_session_id, admin_id) = session_ids(depot)?;
    let body: EnvironmentBody = req
        .parse_json()
        .await
        .map_err(|_| AppError::BadRequest("Invalid environment body.".into()))?;
    let normalized = normalize_body(body)?;
    let count = queries::count_for_admin(&pool, admin_id).await?;
    if count >= MAX_ENVIRONMENTS {
        return Err(AppError::BadRequest(format!(
            "An account may have at most {MAX_ENVIRONMENTS} environments."
        )));
    }
    let row = queries::insert(
        &pool,
        admin_id,
        &normalized.name,
        &normalized.kratos_admin_url,
        &normalized.hydra_admin_url,
        &normalized.hydra_public_url,
        normalized.keto_read_url.as_deref(),
        normalized.keto_write_url.as_deref(),
        normalized.keto_opl_url.as_deref(),
    )
    .await?;
    Ok(Json(view(row)))
}

/// `PUT /api/console/environments/{id}` — replace one of this admin's contours.
#[handler]
pub async fn update_environment(
    req: &mut Request,
    depot: &mut Depot,
) -> Result<Json<EnvironmentView>, AppError> {
    let pool = pool(depot)?;
    let (_session_id, admin_id) = session_ids(depot)?;
    let id = path_id(req)?;
    let body: EnvironmentBody = req
        .parse_json()
        .await
        .map_err(|_| AppError::BadRequest("Invalid environment body.".into()))?;
    let normalized = normalize_body(body)?;
    let row = queries::update(
        &pool,
        admin_id,
        id,
        &normalized.name,
        &normalized.kratos_admin_url,
        &normalized.hydra_admin_url,
        &normalized.hydra_public_url,
        normalized.keto_read_url.as_deref(),
        normalized.keto_write_url.as_deref(),
        normalized.keto_opl_url.as_deref(),
    )
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(Json(view(row)))
}

/// `DELETE /api/console/environments/{id}` — remove a contour. Sessions that
/// pointed at it fall back to Local via `ON DELETE SET NULL`.
#[handler]
pub async fn delete_environment(
    req: &mut Request,
    depot: &mut Depot,
    res: &mut Response,
) -> Result<(), AppError> {
    let pool = pool(depot)?;
    let (_session_id, admin_id) = session_ids(depot)?;
    let id = path_id(req)?;
    if !queries::delete(&pool, admin_id, id).await? {
        return Err(AppError::NotFound);
    }
    res.status_code(StatusCode::NO_CONTENT);
    Ok(())
}

/// `PUT /api/console/environments/active` — point this session at a contour.
/// `{"id": null}` selects the built-in Local contour.
#[handler]
pub async fn set_active(
    req: &mut Request,
    depot: &mut Depot,
) -> Result<Json<serde_json::Value>, AppError> {
    let pool = pool(depot)?;
    let (session_id, admin_id) = session_ids(depot)?;
    let body: ActiveBody = req
        .parse_json()
        .await
        .map_err(|_| AppError::BadRequest("Invalid active-environment body.".into()))?;
    if let Some(id) = body.id {
        if queries::get_owned(&pool, admin_id, id).await?.is_none() {
            return Err(AppError::NotFound);
        }
    }
    queries::set_session_environment(&pool, session_id, admin_id, body.id).await?;
    Ok(Json(serde_json::json!({ "active_id": body.id })))
}
