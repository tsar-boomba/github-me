use std::
    time::Duration
;

use axum::{
    body::{Body, Bytes},
    response::Response,
    routing::get,
    Router,
};
use headers::{CacheControl, HeaderMapExt};
use lambda_http::{run, Error};
use once_cell::sync::OnceCell;
use tower_http::cors::CorsLayer;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};

static TOTAL: OnceCell<Bytes> = OnceCell::new();
async fn total() -> Result<Response, String> {
    let total = if let Some(total) = TOTAL.get() {
        total.clone()
    } else {
        TOTAL
            .set(
                common::get_total_stats()
                    .await
                    .map_err(|e| {
                        tracing::error!("{e:?}");
                        "Failed to get total stats".to_string()
                    })?
                    .into(),
            )
            .ok();
        TOTAL.get().unwrap().clone()
    };

    // 6hrs cache
    let cache_header = CacheControl::new().with_max_age(Duration::from_secs(6 * 60 * 60));
    let mut res = Response::builder().body(Body::from(total)).unwrap();
    res.headers_mut().typed_insert(cache_header);
    Ok(res)
}

static PER_REPO: OnceCell<Bytes> = OnceCell::new();
async fn per_repo() -> Result<Response, String> {
    let total = if let Some(total) = PER_REPO.get() {
        total.clone()
    } else {
        PER_REPO
            .set(
                common::get_per_repo_stats()
                    .await
                    .map_err(|e| {
                        tracing::error!("{e:?}");
                        "Failed to get total stats".to_string()
                    })?
                    .into(),
            )
            .ok();
        PER_REPO.get().unwrap().clone()
    };

    // 6hrs cache
    let cache_header = CacheControl::new().with_max_age(Duration::from_secs(6 * 60 * 60));
    let mut res = Response::builder().body(Body::from(total)).unwrap();
    res.headers_mut().typed_insert(cache_header);
    Ok(res)
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        // disable printing the name of the module in every log line.
        .with_target(false)
        // disabling time is handy because CloudWatch will add the ingestion time.
        .without_time()
        .init();

    let app = Router::new()
        .route("/total", get(total))
        .route("/per-repo", get(per_repo))
        .layer(CorsLayer::permissive());

    run(app).await
}
