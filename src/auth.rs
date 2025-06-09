use std::env;

use axum::{
  extract::FromRequestParts,
  http::{ request::Parts, HeaderMap },
  response::IntoResponse,
  Json,
};
use jsonwebtoken::{ decode, Algorithm, DecodingKey, Validation };
use reqwest::{ Client, StatusCode };
use serde::{ Serialize, Deserialize };
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct LoginRequest {
  email: String,
  password: String,
}

#[derive(Serialize, Deserialize)]
pub struct RegisterRequest {
  email: String,
  password: String,
}

#[derive(Serialize, Deserialize)]
struct AuthResponse {
  access_token: Option<String>,
  refresh_token: Option<String>,
  user: Option<serde_json::Value>,
  error: Option<AuthError>,
}

#[derive(Serialize, Deserialize)]
struct AuthError {
  message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
  pub sub: String,
  pub email: String,
  pub aud: String,
  pub exp: usize,
  pub iat: usize,
  pub role: Option<String>,
}

#[derive(Debug)]
pub struct AuthUser {
  pub user_id: Uuid,
  pub email: String,
  pub audience: String,
  pub client_id: Uuid,
}

fn verify_token(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
  let secret = env::var("SUPABASE_JWT_SECRET").expect("Invalid Supabase JWT Secret");
  let key = DecodingKey::from_secret(secret.as_ref());
  let mut validation = Validation::new(Algorithm::HS256);
  validation.set_audience(&["authenticated"]);

  let token_data = decode::<Claims>(token, &key, &validation);
  match token_data {
    Ok(data) => Ok(data.claims),
    Err(err) => Err(err),
  }
}

impl<S> FromRequestParts<S> for AuthUser where S: Send + Sync {
  type Rejection = (StatusCode, String);

  async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
    let headers = &parts.headers;

    let auth_header = headers
      .get("Authorization")
      .ok_or((StatusCode::UNAUTHORIZED, "Missing Authorization header".to_string()))?;

    let auth_str = auth_header
      .to_str()
      .map_err(|_| (StatusCode::UNAUTHORIZED, "Invalid Authorization header".to_string()))?;

    let token = auth_str
      .strip_prefix("Bearer ")
      .ok_or((StatusCode::UNAUTHORIZED, "Invalid Authorization format".to_string()))?;

    let claims = verify_token(token).map_err(|e| (StatusCode::UNAUTHORIZED, e.to_string()))?;

    let user_id = Uuid::parse_str(&claims.sub).map_err(|_| (
      StatusCode::UNAUTHORIZED,
      "Invalid user ID in token".to_string(),
    ))?;

    let client_id = headers
      .get("X-Client-Sync-Id")
      .and_then(|h| h.to_str().ok())
      .and_then(|s| Uuid::parse_str(s).ok())
      .unwrap_or_else(Uuid::nil);

    Ok(AuthUser {
      user_id,
      email: claims.email.clone(),
      audience: claims.aud,
      client_id,
    })
  }
}

pub async fn login_handler(Json(payload): Json<LoginRequest>) -> impl IntoResponse {
  let supabase_url = env::var("SUPABASE_URL").expect("Invalid Supabase URL");
  let anon_key = env::var("SUPABASE_ANON_KEY").expect("Invalid Supabase Anon Key");
  let client = Client::new();

  let res = client
    .post(format!("{}/auth/v1/token?grant_type=password", supabase_url))
    .header("apikey", &anon_key)
    .header("Content-Type", "application/json")
    .json(&payload)
    .send().await;

  match res {
    Ok(response) => {
      let _status = response.status();
      match response.json::<AuthResponse>().await {
        Ok(body) => {
          if body.access_token.is_some() {
            (StatusCode::OK, Json(body))
          } else {
            (
              StatusCode::UNAUTHORIZED,
              Json(AuthResponse {
                access_token: None,
                refresh_token: None,
                user: None,
                error: Some(AuthError {
                  message: "Invalid credentials".to_string(),
                }),
              }),
            )
          }
        }
        Err(_) =>
          (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(AuthResponse {
              access_token: None,
              refresh_token: None,
              user: None,
              error: Some(AuthError {
                message: "Failed to parse response".to_string(),
              }),
            }),
          ),
      }
    }
    Err(_) =>
      (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AuthResponse {
          access_token: None,
          refresh_token: None,
          user: None,
          error: Some(AuthError {
            message: "Failed to connect to authentication service".to_string(),
          }),
        }),
      ),
  }
}

pub async fn logout_handler(headers: HeaderMap) -> impl IntoResponse {
  let supabase_url = env::var("SUPABASE_URL").unwrap();
  let anon_key = env::var("SUPABASE_ANON_KEY").unwrap();
  let client = reqwest::Client::new();

  let auth_header = match headers.get("Authorization") {
    Some(header_value) => header_value.to_str().ok(),
    None => None,
  };

  let token = match auth_header.and_then(|h| h.strip_prefix("Bearer ")) {
    Some(token) => token,
    None => {
      return (
        StatusCode::UNAUTHORIZED,
        Json(
          serde_json::json!({
                    "error": "Missing or invalid Authorization header"
                })
        ),
      );
    }
  };

  let res = client
    .post(format!("{}/auth/v1/logout", supabase_url))
    .header("apikey", &anon_key)
    .header("Authorization", format!("Bearer {}", token))
    .send().await;

  match res {
    Ok(response) => {
      let status = response.status();
      match status {
        StatusCode::NO_CONTENT =>
          (status, Json(serde_json::json!({"message": "Logged out successfully"}))),
        _ => (status, Json(serde_json::json!({"error": "No user currently logged in"}))),
      }
    }
    Err(_) =>
      (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
                "error": "Failed to logout"
            })),
      ),
  }
}

pub async fn me_handler(user: AuthUser) -> impl IntoResponse {
  Json(
    serde_json::json!({
        "user_id": user.user_id,
        "email": user.email,
        "audience": user.audience
    })
  )
}
