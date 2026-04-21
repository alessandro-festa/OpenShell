// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! HTTP routing (health port and main listener).

use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use serde::Serialize;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Service status.
    pub status: &'static str,

    /// Service version.
    pub version: &'static str,
}

/// Simple health check - returns 200 OK.
async fn health() -> impl IntoResponse {
    StatusCode::OK
}

/// Kubernetes liveness probe.
async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}

/// Kubernetes readiness probe with detailed status.
async fn readyz() -> impl IntoResponse {
    let response = HealthResponse {
        status: "healthy",
        version: openshell_core::VERSION,
    };

    (StatusCode::OK, Json(response))
}

/// Create the health router.
pub fn health_router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
}

/// Plaintext HTTP for [`health_router`].
pub async fn serve_health_listener(listener: TcpListener) -> std::io::Result<()> {
    let app = health_router();
    axum::serve(listener, app).await
}

/// SSH CONNECT, WebSocket tunnel, and browser auth for the main listener.
pub fn http_router(state: Arc<crate::ServerState>) -> Router {
    Router::new()
        .merge(crate::ssh_tunnel::router(state.clone()))
        .merge(crate::ws_tunnel::router(state.clone()))
        .merge(crate::auth::router(state))
}
