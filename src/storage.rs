use std::{ env, path::PathBuf, str::FromStr };
use aws_config::Region;
use aws_sdk_s3::{ config::Credentials, primitives::ByteStream };
use axum::{ body::Body, extract::{ Path, State }, http::StatusCode, response::IntoResponse };
use log::{ error, info, warn };
use axum::{ extract::Multipart };
use tokio_util::io::ReaderStream;

use crate::{
  app_state::AppState,
  auth::AuthUser,
  database::{ check_duplicate, delete_metadata, get_metadata, insert_metadata, FontRecord },
  metadata::extract_metadata,
  sync_engine::{ SyncMessage, SyncSource },
};

static S3_BUCKET: &str = "fonts";

pub async fn connect_s3() -> Result<aws_sdk_s3::Client, Box<dyn std::error::Error>> {
  info!("Connecting to S3 storage...");

  let s3_url = env::var("S3_URL").expect("Invalid s3 storage url");
  let s3_access_key = env::var("S3_ACCESS_KEY").expect("Invalid s3 storage url");
  let s3_secret = env::var("S3_ACCESS_KEY_SECRET").expect("Secret key must be provided");
  let region = env::var("S3_REGION").expect("Region must be provided");

  let cred = Credentials::new(s3_access_key, s3_secret, None, None, "development");
  let s3_config = aws_sdk_s3::config::Builder
    ::new()
    .behavior_version_latest()
    .endpoint_url(s3_url)
    .credentials_provider(cred)
    .region(Region::new(region))
    .force_path_style(true)
    .build();

  let client = aws_sdk_s3::Client::from_conf(s3_config);

  info!("S3 storage connection established.");

  Ok(client)
}

pub async fn upload_font(
  user: AuthUser,
  State(state): State<AppState>,
  mut multipart: Multipart
) -> impl IntoResponse {
  let mut file_name = String::new();
  let mut font_records = Vec::new();
  let mut relative_path = None;

  while
    let Some(field) = multipart
      .next_field().await
      .map_err(|e| { (StatusCode::BAD_REQUEST, format!("Invalid multipart data: {}", e)) })?
  {
    if matches!(field.name(), Some("path")) {
      let path_value = field
        .text().await
        .map_err(|e| { (StatusCode::BAD_REQUEST, format!("Failed to read path text: {}", e)) })?;

      relative_path = Some(path_value);
      continue;
    }

    file_name = field
      .file_name()
      .ok_or((StatusCode::BAD_REQUEST, "Missing file name".to_string()))?
      .to_string();

    let content_type = field
      .content_type()
      .map(|ct| ct.to_string())
      .unwrap_or_else(|| "application/octet-stream".to_string());

    let data = field
      .bytes().await
      .map_err(|e| { (StatusCode::BAD_REQUEST, format!("Failed to read file data: {}", e)) })?;

    let user_key = format!("{}/{}", user.user_id, relative_path.as_deref().unwrap_or(&file_name));

    let (family, subfamily, checksum) = extract_metadata(&data)
      .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid font file: {}", e)))?
      .ok_or((
        StatusCode::BAD_REQUEST,
        "Could not extract font metadata from uploaded file".to_string(),
      ))?;

    info!("Uploading font: family = {}, subfamily = {}", family, subfamily);

    if
      let Some(existing_path) = check_duplicate(
        &state.db_client,
        &user.user_id,
        &checksum,
        &user_key
      ).await.map_err(|e| {
        error!("Error checking for duplicates: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to check for duplicate files".to_string())
      })?
    {
      match state.s3_client.head_object().bucket(S3_BUCKET).key(&existing_path).send().await {
        Ok(_) => {
          info!(
            "Duplicate font detected for user {}: {} (checksum: {})",
            user.email,
            file_name,
            checksum
          );
          return Ok((StatusCode::OK, format!("Duplicate file: {}", existing_path)));
        }
        Err(e) if e.as_service_error().map(|e| e.is_not_found()) == Some(true) => {
          warn!(
            "Database entry found but S3 file missing for user {}: {}",
            user.email,
            existing_path
          );
          delete_metadata(&state.db_client, &user.user_id, &user_key).await.map_err(|e| {
            error!("Failed to remove broken metadata: {}", e);
            (
              StatusCode::INTERNAL_SERVER_ERROR,
              "Broken database state and failed to clean it up".to_string(),
            )
          })?;
        }
        Err(e) => {
          error!("Failed to verify S3 object existence: {}", e);
          return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Error checking file in storage".to_string(),
          ));
        }
      }
    }

    let body = ByteStream::from(data);
    if
      let Err(e) = state.s3_client
        .put_object()
        .bucket(S3_BUCKET)
        .key(&user_key)
        .content_type(content_type)
        .body(body)
        .send().await
    {
      if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
        if service_err.raw().status().as_u16() == 403 {
          return Err((
            StatusCode::FORBIDDEN,
            "Access denied: insufficient permissions to upload files".to_string(),
          ));
        } else if service_err.raw().status().as_u16() == 404 {
          return Err((StatusCode::NOT_FOUND, "Bucket not found or does not exist".to_string()));
        }
      }

      return Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to upload file '{}': Server error", &file_name),
      ));
    }

    font_records.push(FontRecord {
      font_family: family,
      font_subfamily: subfamily,
      object_path: user_key.clone(),
      checksum,
    });

    info!("User {} uploaded file: {}", user.email, file_name);
  }

  if !font_records.is_empty() {
    if let Err(e) = insert_metadata(&state.db_client, &user.user_id, &font_records).await {
      error!("Failed to insert font metadata: {}", e);

      return Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        "File uploaded but failed to save metadata".to_string(),
      ));
    }
    info!("Successfully inserted {} font records into database", font_records.len());
  }

  if let Some(_record) = font_records.first() {
    let path_result = PathBuf::from_str(relative_path.as_deref().unwrap_or(&file_name));
    let path = path_result.expect("Failed to convert to PathBuf");

    let sync_msg = SyncMessage::ObjectCreated { path, source: SyncSource::Server };

    if let Err(e) = state.notify_tx.send(sync_msg).await {
      error!("Failed to notify about new file: {}", e);
    }
  }

  Ok((StatusCode::OK, format!("File {} uploaded successfully", file_name)))
}

pub async fn get_font(
  user: AuthUser,
  Path(key): Path<String>,
  State(state): State<AppState>
) -> impl IntoResponse {
  let user_key = format!("{}/{}", user.user_id, key);

  info!("User {} downloading file: {}", user.email, key);
  let object = match state.s3_client.get_object().bucket(S3_BUCKET).key(&user_key).send().await {
    Ok(obj) => obj,
    Err(e) => {
      if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
        if service_err.err().is_no_such_key() {
          error!("File key does not exist: {}", key);
          return Err((StatusCode::NOT_FOUND, Body::from(format!("File '{}' does not exist", key))));
        }
      }

      error!("Failed to retrieve file {}: {}", key, e);
      return Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        Body::from(format!("Unable to retrieve file '{}': Server error", key)),
      ));
    }
  };

  let body_stream = object.body;
  let stream = body_stream.into_async_read();
  let reader_stream = ReaderStream::new(stream);

  Ok((StatusCode::OK, Body::from_stream(reader_stream)))
}

pub async fn delete_font(
  user: AuthUser,
  Path(key): Path<String>,
  State(state): State<AppState>
) -> impl IntoResponse {
  let user_key = format!("{}/{}", user.user_id, key);

  info!("User {} deleting file: {}", user.email, key);

  match state.s3_client.head_object().bucket(S3_BUCKET).key(&user_key).send().await {
    Ok(_) => {
      match state.s3_client.delete_object().bucket(S3_BUCKET).key(&user_key).send().await {
        Ok(_) => {
          // Delete metadata
          match delete_metadata(&state.db_client, &user.user_id, &user_key).await {
            Ok(rows_deleted) => {
              let sync_msg = SyncMessage::ObjectDeleted {
                path: key.clone().into(),
                source: SyncSource::Server,
              };
              tokio::spawn({
                let notify_tx = state.notify_tx.clone();
                async move {
                  if let Err(e) = notify_tx.send(sync_msg).await {
                    error!("Failed to notify about deleted file: {}", e);
                  }
                }
              });
              info!("Deleted {} metadata record(s) for {}", rows_deleted, &user_key);
            }
            Err(e) => {
              error!("Metadata deletion failed: {}", e);
            }
          }
          info!("Successfully deleted file: {}", key);
          (StatusCode::OK, "File deleted successfully").into_response()
        }
        Err(e) => {
          let error_msg = format!("Failed to delete file '{}': {}", key, e);
          error!("Failed to delete existing file {}: {}", key, e);
          (StatusCode::INTERNAL_SERVER_ERROR, error_msg).into_response()
        }
      }
    }
    Err(e) => {
      if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
        if service_err.err().is_not_found() {
          let error_msg = format!("File '{}' does not exist", key);
          error!("Attempted to delete non-existent file: {}", key);
          return (StatusCode::NOT_FOUND, error_msg).into_response();
        }
      }

      let error_msg = format!("Unable to access file '{}': {}", key, e);
      error!("Failed to check if file {} exists: {}", key, e);
      (StatusCode::INTERNAL_SERVER_ERROR, error_msg).into_response()
    }
  }
}

pub async fn list_fonts(user: AuthUser, State(state): State<AppState>) -> impl IntoResponse {
  let user_prefix = format!("{}/", user.user_id);

  let s3_result = state.s3_client
    .list_objects_v2()
    .bucket(S3_BUCKET)
    .prefix(&user_prefix)
    .send().await;

  let db_result = get_metadata(&state.db_client, &user.user_id).await;

  match (s3_result, db_result) {
    (Ok(s3_res), Ok(fonts)) => {
      let s3_keys: std::collections::HashSet<_> = s3_res
        .contents()
        .iter()
        .filter_map(|o| o.key())
        .map(|k| k.to_string())
        .collect();

      let matched: Vec<FontRecord> = fonts
        .into_iter()
        .filter(|f| s3_keys.contains(&f.object_path))
        .collect();

      axum::Json(matched).into_response()
    }
    (Err(s3_err), _) => {
      (StatusCode::INTERNAL_SERVER_ERROR, format!("S3 error: {}", s3_err)).into_response()
    }
    (_, Err(db_err)) => {
      (StatusCode::INTERNAL_SERVER_ERROR, format!("DB error: {}", db_err)).into_response()
    }
  }
}
