use std::sync::Arc;

use crate::storage::{ put_object, list_objects, delete_object };
use axum::{
    extract::DefaultBodyLimit,
    http::{ HeaderValue, Method },
    routing::{ delete, get, put },
    Router,
};
use storage::{ connect_s3, get_object };
use tower_http::cors::CorsLayer;

mod database;
mod storage;

const FILE_SIZE_LIMIT: usize = 100 * 1024 * 1024;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    println!("Hello, world!");

    let client = Arc::new(connect_s3().await.expect("Failed to connect to S3"));

    let cors = CorsLayer::new()
        .allow_origin("http://localhost:3000".parse::<HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/upload", put(put_object))
        .route("/files/{*key}", get(get_object))
        .route("/files/{*key}", delete(delete_object))
        .route("/files", get(list_objects))
        .with_state(client)
        .layer(cors)
        .layer(DefaultBodyLimit::max(FILE_SIZE_LIMIT));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app).await.unwrap();
}
