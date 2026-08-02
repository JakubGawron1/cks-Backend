use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::auth::extractor::{ensure_roles, AuthUser};
use crate::error::{AppError, AppResult};
use crate::models::club::{CmsBlock, CmsPage, CmsStatus, LogLevel};
use crate::models::role::Role;
use crate::models::user::{ErrorBody, OkResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, ToSchema)]
pub struct CmsPageBody {
    pub slug: String,
    pub title: String,
    pub status: CmsStatus,
    pub blocks: Vec<CmsBlock>,
}

#[utoipa::path(
    get,
    path = "/api/cms/pages",
    responses(
        (status = 200, description = "Lista stron CMS", body = Vec<CmsPage>),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_cms_pages(
    State(state): State<AppState>,
    auth: AuthUser,
) -> AppResult<Json<Vec<CmsPage>>> {
    ensure_roles(&auth, &[Role::Admin])?;
    Ok(Json(state.db.list_cms_pages().await?))
}

#[utoipa::path(
    post,
    path = "/api/cms/pages",
    request_body = CmsPageBody,
    responses(
        (status = 200, description = "Utworzono stronę CMS", body = CmsPage),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_cms_page(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CmsPageBody>,
) -> AppResult<Json<CmsPage>> {
    ensure_roles(&auth, &[Role::Admin])?;
    validate_page(&body)?;

    let now = chrono::Utc::now().to_rfc3339();
    let page = CmsPage {
        id: uuid::Uuid::new_v4().to_string(),
        slug: body.slug.trim().to_string(),
        title: body.title.trim().to_string(),
        status: body.status,
        blocks: body.blocks,
        created_at: now.clone(),
        updated_at: now,
    };
    state.db.upsert_cms_page(page.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "cms",
        &format!("Utworzono stronę CMS /{}", page.slug),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(page))
}

#[utoipa::path(
    patch,
    path = "/api/cms/pages/{id}",
    params(("id" = String, Path, description = "ID strony CMS")),
    request_body = CmsPageBody,
    responses(
        (status = 200, description = "Zaktualizowano stronę CMS", body = CmsPage),
        (status = 400, description = "Nieprawidłowe dane wejściowe", body = ErrorBody),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
        (status = 404, description = "Strona CMS nie istnieje", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_cms_page(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CmsPageBody>,
) -> AppResult<Json<CmsPage>> {
    ensure_roles(&auth, &[Role::Admin])?;
    validate_page(&body)?;

    let existing = state
        .db
        .get_cms_page(&id).await?
        .ok_or_else(|| AppError::NotFound("Strona CMS nie istnieje.".into()))?;

    let page = CmsPage {
        id: existing.id,
        slug: body.slug.trim().to_string(),
        title: body.title.trim().to_string(),
        status: body.status,
        blocks: body.blocks,
        created_at: existing.created_at,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    state.db.upsert_cms_page(page.clone()).await?;
    state.db.append_log(
        LogLevel::Info,
        "cms",
        &format!("Zaktualizowano stronę CMS /{}", page.slug),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(page))
}

#[utoipa::path(
    delete,
    path = "/api/cms/pages/{id}",
    params(("id" = String, Path, description = "ID strony CMS")),
    responses(
        (status = 200, description = "Usunięto stronę CMS", body = OkResponse),
        (status = 401, description = "Unauthorized", body = ErrorBody),
        (status = 403, description = "Forbidden", body = ErrorBody),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_cms_page(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> AppResult<Json<OkResponse>> {
    ensure_roles(&auth, &[Role::Admin])?;
    state.db.delete_cms_page(&id).await?;
    state.db.append_log(
        LogLevel::Warn,
        "cms",
        &format!("Usunięto stronę CMS {id}"),
        Some(&auth.user.id),
    ).await?;
    Ok(Json(OkResponse { ok: true }))
}

fn validate_page(body: &CmsPageBody) -> AppResult<()> {
    if body.slug.trim().is_empty() || body.title.trim().is_empty() {
        return Err(AppError::BadRequest("Slug i tytuł są wymagane.".into()));
    }
    Ok(())
}
