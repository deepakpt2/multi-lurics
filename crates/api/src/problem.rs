//! RFC 7807 / 9457 problem details. `code` is the stable key clients localize;
//! `title` is an English fallback only.

use crate::middleware::current_request_id;
use application::AppError;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use domain::FieldViolation;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

pub const CONTENT_TYPE: &str = "application/problem+json";

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FieldError {
    /// Request field name (snake_case).
    #[schema(example = "email")]
    pub field: String,
    /// Stable violation code, e.g. `required`, `too_short`, `invalid_email`.
    #[schema(example = "invalid_email")]
    pub code: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    #[schema(example = "/problems/validation_failed")]
    pub type_: String,
    pub title: String,
    pub status: u16,
    /// Stable machine-readable error code.
    #[schema(example = "validation_failed")]
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Present for `validation_failed`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<FieldError>,
    /// Present on `409` revision conflicts: the revision the client must rebase onto.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_revision_id: Option<Uuid>,
    /// Present on `409 version_conflict`: the record's current `version`; reload, then reapply.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_version: Option<i32>,
    /// Present on `409 duplicate_identifier` when known: the record already holding the identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct ApiError(pub Box<Problem>);

impl ApiError {
    pub fn new(status: StatusCode, code: &str, title: &str) -> Self {
        Self(Box::new(Problem {
            type_: format!("/problems/{code}"),
            title: title.to_owned(),
            status: status.as_u16(),
            code: code.to_owned(),
            detail: None,
            request_id: None,
            errors: Vec::new(),
            current_revision_id: None,
            current_version: None,
            existing_id: None,
        }))
    }

    fn with_errors(mut self, v: Vec<FieldViolation>) -> Self {
        self.0.errors = v
            .into_iter()
            .map(|f| FieldError {
                field: f.field.into(),
                code: f.code.into(),
            })
            .collect();
        self
    }
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        use StatusCode as S;
        match e {
            AppError::Validation(v) => {
                Self::new(S::UNPROCESSABLE_ENTITY, "validation_failed", "Validation failed").with_errors(v)
            }
            AppError::BadRequest(code) => Self::new(S::BAD_REQUEST, code, "Bad request"),
            AppError::Unauthenticated => Self::new(S::UNAUTHORIZED, "unauthenticated", "Authentication required"),
            AppError::InvalidCredentials => {
                Self::new(S::UNAUTHORIZED, "invalid_credentials", "Email or password is incorrect")
            }
            AppError::EmailNotVerified => Self::new(S::FORBIDDEN, "email_not_verified", "Email address not verified"),
            AppError::Forbidden => Self::new(S::FORBIDDEN, "forbidden", "Forbidden"),
            AppError::InvalidToken => Self::new(S::BAD_REQUEST, "invalid_token", "Link is invalid or has expired"),
            AppError::NotFound { entity } => {
                let mut p = Self::new(S::NOT_FOUND, "not_found", "Not found");
                p.0.detail = Some(format!("{entity} not found"));
                p
            }
            AppError::Conflict {
                code,
                current_revision_id,
            } => {
                let mut p = Self::new(S::CONFLICT, code, "Conflict");
                p.0.current_revision_id = current_revision_id;
                p
            }
            AppError::StaleVersion { current_version } => {
                let mut p = Self::new(S::CONFLICT, "version_conflict", "Conflict");
                p.0.current_version = Some(current_version);
                p
            }
            AppError::Duplicate { field, existing_id } => {
                let mut p = Self::new(S::CONFLICT, "duplicate_identifier", "Duplicate identifier").with_errors(vec![
                    FieldViolation {
                        field,
                        code: "duplicate",
                    },
                ]);
                p.0.existing_id = existing_id;
                p
            }
            AppError::Taken { code, existing_id } => {
                let mut p = Self::new(S::CONFLICT, code, "Conflict");
                p.0.existing_id = existing_id;
                p
            }
            AppError::Unavailable => Self::new(S::SERVICE_UNAVAILABLE, "unavailable", "Service unavailable"),
            AppError::Internal(err) => {
                // Full error goes to logs (correlated by request id), never to the client.
                tracing::error!(error = ?err, "internal error");
                Self::new(S::INTERNAL_SERVER_ERROR, "internal", "Internal server error")
            }
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        match r {
            JsonRejection::MissingJsonContentType(_) => Self::new(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_media_type",
                "Expected application/json",
            ),
            JsonRejection::JsonSyntaxError(_) => Self::new(StatusCode::BAD_REQUEST, "malformed_json", "Malformed JSON"),
            _ => Self::new(
                StatusCode::BAD_REQUEST,
                "invalid_body",
                "Request body has the wrong shape",
            ),
        }
    }
}

impl From<QueryRejection> for ApiError {
    fn from(_: QueryRejection) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_query", "Invalid query parameters")
    }
}

/// A malformed id in the path cannot name an existing resource.
impl From<PathRejection> for ApiError {
    fn from(_: PathRejection) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found")
    }
}

impl IntoResponse for ApiError {
    fn into_response(mut self) -> Response {
        self.0.request_id = current_request_id();
        let status = StatusCode::from_u16(self.0.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut res = (status, axum::Json(*self.0)).into_response();
        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE));
        res
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
