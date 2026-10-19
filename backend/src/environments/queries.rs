//! Runtime SQL for admin contours. These queries are not `sqlx::query!`
//! macros, so they do not need a prepared `.sqlx` cache entry.

use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, sqlx::FromRow)]
pub struct EnvironmentRow {
    pub id: Uuid,
    pub name: String,
    pub kratos_admin_url: String,
    pub hydra_admin_url: String,
    pub hydra_public_url: String,
    pub keto_read_url: Option<String>,
    pub keto_write_url: Option<String>,
    pub keto_opl_url: Option<String>,
}

fn map_unique(e: sqlx::Error) -> AppError {
    match e.as_database_error().and_then(|d| d.code()).as_deref() {
        Some("23505") => {
            AppError::BadRequest("An environment with this name already exists.".into())
        }
        _ => AppError::Db(e),
    }
}

pub async fn session_environment_id(
    pool: &PgPool,
    session_id: Uuid,
) -> Result<Option<Uuid>, AppError> {
    let row: Option<(Option<Uuid>,)> =
        sqlx::query_as("SELECT environment_id FROM sessions WHERE id = $1")
            .bind(session_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|r| r.0))
}

pub async fn set_session_environment(
    pool: &PgPool,
    session_id: Uuid,
    admin_id: Uuid,
    environment_id: Option<Uuid>,
) -> Result<(), AppError> {
    sqlx::query("UPDATE sessions SET environment_id = $1 WHERE id = $2 AND admin_id = $3")
        .bind(environment_id)
        .bind(session_id)
        .bind(admin_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_for_admin(
    pool: &PgPool,
    admin_id: Uuid,
) -> Result<Vec<EnvironmentRow>, AppError> {
    let rows = sqlx::query_as::<_, EnvironmentRow>(
        r#"
        SELECT id, name, kratos_admin_url, hydra_admin_url, hydra_public_url,
               keto_read_url, keto_write_url, keto_opl_url
        FROM admin_environments
        WHERE admin_id = $1
        ORDER BY name
        "#,
    )
    .bind(admin_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn count_for_admin(pool: &PgPool, admin_id: Uuid) -> Result<i64, AppError> {
    let (count,): (i64,) =
        sqlx::query_as("SELECT count(*)::bigint FROM admin_environments WHERE admin_id = $1")
            .bind(admin_id)
            .fetch_one(pool)
            .await?;
    Ok(count)
}

pub async fn get_owned(
    pool: &PgPool,
    admin_id: Uuid,
    id: Uuid,
) -> Result<Option<EnvironmentRow>, AppError> {
    let row = sqlx::query_as::<_, EnvironmentRow>(
        r#"
        SELECT id, name, kratos_admin_url, hydra_admin_url, hydra_public_url,
               keto_read_url, keto_write_url, keto_opl_url
        FROM admin_environments
        WHERE id = $1 AND admin_id = $2
        "#,
    )
    .bind(id)
    .bind(admin_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn insert(
    pool: &PgPool,
    admin_id: Uuid,
    name: &str,
    kratos_admin_url: &str,
    hydra_admin_url: &str,
    hydra_public_url: &str,
    keto_read_url: Option<&str>,
    keto_write_url: Option<&str>,
    keto_opl_url: Option<&str>,
) -> Result<EnvironmentRow, AppError> {
    let row = sqlx::query_as::<_, EnvironmentRow>(
        r#"
        INSERT INTO admin_environments (
            admin_id, name, kratos_admin_url, hydra_admin_url, hydra_public_url,
            keto_read_url, keto_write_url, keto_opl_url
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, name, kratos_admin_url, hydra_admin_url, hydra_public_url,
                  keto_read_url, keto_write_url, keto_opl_url
        "#,
    )
    .bind(admin_id)
    .bind(name)
    .bind(kratos_admin_url)
    .bind(hydra_admin_url)
    .bind(hydra_public_url)
    .bind(keto_read_url)
    .bind(keto_write_url)
    .bind(keto_opl_url)
    .fetch_one(pool)
    .await
    .map_err(map_unique)?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn update(
    pool: &PgPool,
    admin_id: Uuid,
    id: Uuid,
    name: &str,
    kratos_admin_url: &str,
    hydra_admin_url: &str,
    hydra_public_url: &str,
    keto_read_url: Option<&str>,
    keto_write_url: Option<&str>,
    keto_opl_url: Option<&str>,
) -> Result<Option<EnvironmentRow>, AppError> {
    let row = sqlx::query_as::<_, EnvironmentRow>(
        r#"
        UPDATE admin_environments
        SET name = $3,
            kratos_admin_url = $4,
            hydra_admin_url = $5,
            hydra_public_url = $6,
            keto_read_url = $7,
            keto_write_url = $8,
            keto_opl_url = $9,
            updated_at = now()
        WHERE id = $1 AND admin_id = $2
        RETURNING id, name, kratos_admin_url, hydra_admin_url, hydra_public_url,
                  keto_read_url, keto_write_url, keto_opl_url
        "#,
    )
    .bind(id)
    .bind(admin_id)
    .bind(name)
    .bind(kratos_admin_url)
    .bind(hydra_admin_url)
    .bind(hydra_public_url)
    .bind(keto_read_url)
    .bind(keto_write_url)
    .bind(keto_opl_url)
    .fetch_optional(pool)
    .await
    .map_err(map_unique)?;
    Ok(row)
}

pub async fn delete(pool: &PgPool, admin_id: Uuid, id: Uuid) -> Result<bool, AppError> {
    let result = sqlx::query("DELETE FROM admin_environments WHERE id = $1 AND admin_id = $2")
        .bind(id)
        .bind(admin_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() == 1)
}
