use crate::storage::{ get_object, put_object, list_objects, delete_object };
use crate::auth::login_handler;
use app_state::create_app_state;
use auth::{ logout_handler, me_handler };
use axum::{
    extract::DefaultBodyLimit,
    http::{ HeaderValue, Method, StatusCode },
    response::IntoResponse,
    routing::{ delete, get, post, put },
    Router,
};
use tower_http::cors::CorsLayer;

mod database;
mod storage;
mod auth;
mod metadata;
mod app_state;

const FILE_SIZE_LIMIT: usize = 100 * 1024 * 1024;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();
    env_logger::init();

    let state = create_app_state().await.expect("Failed to create app state");

    let cors = CorsLayer::new()
        .allow_origin("http://localhost:3000".parse::<HeaderValue>().unwrap())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers(["content-type".parse().unwrap(), "authorization".parse().unwrap()]);

    let app = Router::new()
        .route("/", get(hello_world))

        .route("/auth/login", post(login_handler))
        .route("/auth/logout", post(logout_handler))
        .route("/auth/me", get(me_handler))

        .route("/upload", put(put_object))
        .route("/files/{*key}", get(get_object))
        .route("/files/{*key}", delete(delete_object))
        .route("/files", get(list_objects))

        .with_state(state)
        .layer(cors)
        .layer(DefaultBodyLimit::max(FILE_SIZE_LIMIT));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

async fn hello_world() -> impl IntoResponse {
    (StatusCode::OK, format!("Hello, world!"))
}
