use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

#[derive(Clone)]
pub struct RequireToken {
    token: String,
}

impl RequireToken {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

pub async fn middleware(
    State(auth): State<RequireToken>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if auth.token.is_empty() {
        return Ok(next.run(req).await);
    }
    let Some(h) = req.headers().get(axum::http::header::AUTHORIZATION) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let Ok(s) = h.to_str() else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    let bearer = s.strip_prefix("Bearer ").unwrap_or(s);
    if bearer != auth.token {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}
