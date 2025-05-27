use std::sync::Arc;

use crate::storage::{ put_object, list_objects, delete_object };
use axum::{ routing::{ delete, get, put }, Router };
use storage::{ connect_s3, get_object };

mod database;
mod storage;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    println!("Hello, world!");

    let client = Arc::new(connect_s3().await.expect("Failed to connect to S3"));

    let app = Router::new()
        .route("/upload", put(put_object))
        .route("/files/{*key}", get(get_object))
        .route("/files/{*key}", delete(delete_object))
        .route("/files", get(list_objects))
        .with_state(client);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app).await.unwrap();
}
