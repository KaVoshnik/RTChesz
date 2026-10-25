use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::{
    Json,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Request},
    http::request::Parts,
};
use serde::de::DeserializeOwned;

use crate::{error::AppError, state::AppState};

/// Как `axum::Json`, но при ошибке разбора отвечает нашим JSON-ом с полем `message`
/// (а не plain text), чтобы фронт мог показать понятную ошибку.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => {
                tracing::debug!(%rejection, "invalid json body");
                Err(AppError::bad_request("Некорректный запрос"))
            }
        }
    }
}

/// IP клиента (для лимитов запросов).
///
/// За обратным прокси (`TRUST_PROXY=true`) берётся последний адрес из
/// `X-Forwarded-For` - тот, что дописал наш прокси. Первые значения
/// подконтрольны клиенту и подделываются, поэтому им верить нельзя.
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if state.config.trust_proxy
            && let Some(ip) = parts
                .headers
                .get("x-forwarded-for")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.rsplit(',').next())
                .and_then(|value| value.trim().parse::<IpAddr>().ok())
        {
            return Ok(ClientIp(ip));
        }

        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));

        Ok(ClientIp(ip))
    }
}
