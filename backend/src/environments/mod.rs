//! Per-admin Ory contours (test / prod) and the session switch that points
//! Kratos, Hydra, and optionally Keto admin calls at one of them.
//!
//! The process env (`KRATOS_ADMIN_URL`, …) stays the built-in "Local" contour.
//! Extra contours are rows on the logged-in admin. The active row id is stored
//! on the session, and [`apply_environment`] retargets the depot `OryClients`
//! for that request only.
//!
//! Local files (service YAML, identity schema, permission model, account
//! experience theme) are NOT part of a remote contour. Mutating those routes
//! while a remote contour is active is rejected so a save cannot rewrite the
//! console's own deployment while the UI is showing another stack.

mod queries;
mod routes;

use salvo::http::StatusCode;
use salvo::prelude::*;

use crate::auth::middleware::ApiKeyPrincipal;
use crate::db::models::Session;
use crate::error::AppError;
use crate::ory::clients::OryClients;

pub use routes::{
    create_environment, delete_environment, list_environments, set_active, update_environment,
};

/// How many contours one admin account may store.
const MAX_ENVIRONMENTS: i64 = 20;

/// Local-file editors. These restart or rewrite THIS console's deployment, so
/// they must not run while the session is aimed at another contour.
const LOCAL_FILE_PREFIXES: &[&str] = &[
    "/api/config/",
    "/api/kratos/identity-schema",
    "/api/kratos/smtp-connection",
    "/api/keto/permission-model",
    "/api/account-experience/theme",
    "/api/account-experience/translations",
];

/// Normalize an operator-entered admin base URL to `scheme://host[:port]`.
///
/// Rejects credentials, query strings, fragments, and any path. Private and
/// docker-internal hosts are allowed: a test contour often lives on a private
/// network. Cloud-metadata literals are rejected.
pub fn normalize_admin_base_url(raw: &str) -> Result<String, AppError> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(AppError::BadRequest("URL is required.".into()));
    }
    if raw.len() > 512 {
        return Err(AppError::BadRequest("URL is too long.".into()));
    }
    let parsed =
        url::Url::parse(raw).map_err(|_| AppError::BadRequest("URL is not valid.".into()))?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return Err(AppError::BadRequest("URL must use http or https.".into())),
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::BadRequest(
            "URL must not include credentials.".into(),
        ));
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(AppError::BadRequest(
            "URL must not include a query or fragment.".into(),
        ));
    }
    let path = parsed.path();
    if path != "/" && !path.is_empty() {
        return Err(AppError::BadRequest(
            "URL must be an origin only (scheme://host:port), with no path.".into(),
        ));
    }
    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| AppError::BadRequest("URL must include a host.".into()))?;
    let host_l = host.to_ascii_lowercase();
    if is_metadata_host(&host_l) {
        return Err(AppError::BadRequest("This host is not allowed.".into()));
    }
    match parsed.origin() {
        url::Origin::Tuple(..) => Ok(parsed.origin().ascii_serialization()),
        _ => Err(AppError::BadRequest("URL is not valid.".into())),
    }
}

fn is_metadata_host(host: &str) -> bool {
    matches!(
        host,
        "169.254.169.254" | "metadata.google.internal" | "metadata.google.com"
    )
}

pub fn normalize_name(raw: &str) -> Result<String, AppError> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 {
        return Err(AppError::BadRequest("Name is required.".into()));
    }
    if chars > 64 {
        return Err(AppError::BadRequest(
            "Name must be at most 64 characters.".into(),
        ));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(AppError::BadRequest(
            "Name cannot contain control characters.".into(),
        ));
    }
    Ok(name.to_owned())
}

/// Blank optional URL -> None. Keto read, write, and OPL must be all set or
/// all omitted so a contour cannot split one Keto across two networks.
pub fn normalize_keto(
    read: Option<&str>,
    write: Option<&str>,
    opl: Option<&str>,
) -> Result<Option<(String, String, String)>, AppError> {
    let read = blank_to_none(read);
    let write = blank_to_none(write);
    let opl = blank_to_none(opl);
    let present = [read.is_some(), write.is_some(), opl.is_some()]
        .into_iter()
        .filter(|p| *p)
        .count();
    if present == 0 {
        return Ok(None);
    }
    if present != 3 {
        return Err(AppError::BadRequest(
            "Keto read, write, and OPL URLs must be set together, or all left empty.".into(),
        ));
    }
    Ok(Some((
        normalize_admin_base_url(read.unwrap())?,
        normalize_admin_base_url(write.unwrap())?,
        normalize_admin_base_url(opl.unwrap())?,
    )))
}

fn blank_to_none(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|s| !s.is_empty())
}

fn is_safe_method(method: &salvo::http::Method) -> bool {
    matches!(
        *method,
        salvo::http::Method::GET | salvo::http::Method::HEAD | salvo::http::Method::OPTIONS
    )
}

fn is_local_file_write(method: &salvo::http::Method, path: &str) -> bool {
    if is_safe_method(method) {
        return false;
    }
    LOCAL_FILE_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

fn pool(depot: &mut Depot) -> Result<sqlx::PgPool, AppError> {
    Ok(depot
        .obtain::<sqlx::PgPool>()
        .map_err(|_| AppError::Internal("pool missing from depot".into()))?
        .clone())
}

fn deny(res: &mut Response, ctrl: &mut FlowCtrl, status: StatusCode, code: &'static str) {
    res.status_code(status);
    res.render(Json(serde_json::json!({ "error": code })));
    ctrl.skip_rest();
}

/// After auth: if this session selected a contour, retarget `OryClients`.
/// API-key requests keep the process-default clients.
#[handler]
pub async fn apply_environment(
    req: &mut Request,
    depot: &mut Depot,
    res: &mut Response,
    ctrl: &mut FlowCtrl,
) {
    if depot.obtain::<ApiKeyPrincipal>().is_ok() {
        return;
    }
    let (session_id, admin_id) = match depot.obtain::<Session>() {
        Ok(session) => (session.id, session.admin_id),
        Err(_) => return,
    };
    let pool = match pool(depot) {
        Ok(pool) => pool,
        Err(_) => {
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            )
        }
    };

    let environment_id = match queries::session_environment_id(&pool, session_id).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, "failed to read session contour");
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            );
        }
    };
    let Some(environment_id) = environment_id else {
        return;
    };

    let row = match queries::get_owned(&pool, admin_id, environment_id).await {
        Ok(Some(row)) => row,
        Ok(None) => return,
        Err(e) => {
            tracing::error!(error = %e, "failed to load session contour");
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            );
        }
    };

    let path = req.uri().path().to_owned();
    if is_local_file_write(req.method(), &path) {
        res.status_code(StatusCode::CONFLICT);
        res.render(Json(serde_json::json!({
            "error": "conflict",
            "message": "Configuration files belong to this console's local deployment. Switch the contour to Local before saving."
        })));
        ctrl.skip_rest();
        return;
    }

    let kratos = match normalize_admin_base_url(&row.kratos_admin_url) {
        Ok(url) => url,
        Err(_) => {
            tracing::error!(environment_id = %environment_id, "stored kratos URL is invalid");
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            );
        }
    };
    let hydra = match normalize_admin_base_url(&row.hydra_admin_url) {
        Ok(url) => url,
        Err(_) => {
            tracing::error!(environment_id = %environment_id, "stored hydra admin URL is invalid");
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            );
        }
    };
    let hydra_public = match normalize_admin_base_url(&row.hydra_public_url) {
        Ok(url) => url,
        Err(_) => {
            tracing::error!(environment_id = %environment_id, "stored hydra public URL is invalid");
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            );
        }
    };
    let keto = match normalize_keto(
        row.keto_read_url.as_deref(),
        row.keto_write_url.as_deref(),
        row.keto_opl_url.as_deref(),
    ) {
        Ok(urls) => urls,
        Err(_) => {
            tracing::error!(environment_id = %environment_id, "stored keto URLs are invalid");
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            );
        }
    };

    let clients = match depot.obtain::<OryClients>() {
        Ok(clients) => clients.clone(),
        Err(_) => {
            return deny(
                res,
                ctrl,
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
            )
        }
    };
    let (keto_read, keto_write, keto_opl) = match &keto {
        Some((read, write, opl)) => (
            Some(read.as_str()),
            Some(write.as_str()),
            Some(opl.as_str()),
        ),
        None => (None, None, None),
    };
    let retargeted = clients.retarget(
        &kratos,
        &hydra,
        &hydra_public,
        keto_read,
        keto_write,
        keto_opl,
    );
    depot.inject(retargeted);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bare_http_origin_and_drops_trailing_slash() {
        assert_eq!(
            normalize_admin_base_url("http://kratos:4434/").unwrap(),
            "http://kratos:4434"
        );
    }

    #[test]
    fn accepts_https_with_explicit_port() {
        assert_eq!(
            normalize_admin_base_url("https://hydra.example:4445").unwrap(),
            "https://hydra.example:4445"
        );
    }

    #[test]
    fn rejects_path_credentials_and_metadata() {
        assert!(normalize_admin_base_url("http://kratos:4434/admin").is_err());
        assert!(normalize_admin_base_url("http://user:pass@kratos:4434").is_err());
        assert!(normalize_admin_base_url("http://169.254.169.254").is_err());
        assert!(normalize_admin_base_url("file:///etc/passwd").is_err());
        assert!(normalize_admin_base_url("").is_err());
    }

    #[test]
    fn keto_urls_are_all_or_nothing() {
        assert!(normalize_keto(None, None, None).unwrap().is_none());
        assert!(normalize_keto(Some("http://keto:4466"), None, None).is_err());
        let urls = normalize_keto(
            Some("http://keto:4466"),
            Some("http://keto:4467/"),
            Some("http://keto:4469"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(urls.1, "http://keto:4467");
    }

    #[test]
    fn local_file_writes_are_detected() {
        use salvo::http::Method;
        assert!(is_local_file_write(
            &Method::PUT,
            "/api/config/kratos/methods"
        ));
        assert!(!is_local_file_write(
            &Method::GET,
            "/api/config/kratos/methods"
        ));
        assert!(!is_local_file_write(
            &Method::DELETE,
            "/api/kratos/identities/abc"
        ));
    }

    #[test]
    fn name_is_trimmed_and_bounded() {
        assert_eq!(normalize_name("  Prod  ").unwrap(), "Prod");
        assert!(normalize_name("").is_err());
        assert!(normalize_name(&"a".repeat(65)).is_err());
    }
}
