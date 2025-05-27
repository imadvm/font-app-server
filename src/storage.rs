use std::{ env, sync::Arc };
use aws_config::Region;
use aws_sdk_s3::{ config::Credentials, primitives::ByteStream };
use axum::{ body::Body, extract::{ Path, State }, http::StatusCode, response::IntoResponse };
use log::{ info, error };
use axum::{ extract::Multipart };
use tokio_util::io::ReaderStream;

type AppState = Arc<aws_sdk_s3::Client>;

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

pub async fn put_object(
    State(client): State<AppState>,
    mut multipart: Multipart
) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.unwrap() {
        let file_name = field.file_name().unwrap().to_string();
        let content_type = field
            .content_type()
            .map(|ct| ct.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let data = field.bytes().await.unwrap();

        let body = ByteStream::from(data);
        if
            let Err(e) = client
                .put_object()
                .bucket(S3_BUCKET)
                .key(&file_name)
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
                    return Err((
                        StatusCode::NOT_FOUND,
                        "Bucket not found or does not exist".to_string(),
                    ));
                }
            }

            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to upload file '{}': Server error", &file_name),
            ));
        }

        info!("Uploaded file: {}", &file_name);
    }

    Ok((StatusCode::OK, format!("File uploaded")))
}

pub async fn get_object(
    Path(key): Path<String>,
    State(client): State<AppState>
) -> impl IntoResponse {
    info!("Downloading file: {}", key);

    let object = match client.get_object().bucket(S3_BUCKET).key(&key).send().await {
        Ok(obj) => obj,
        Err(e) => {
            if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                if service_err.err().is_no_such_key() {
                    error!("File key does not exist: {}", key);
                    return Err((
                        StatusCode::NOT_FOUND,
                        Body::from(format!("File '{}' does not exist", key)),
                    ));
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

pub async fn delete_object(
    Path(key): Path<String>,
    State(client): State<AppState>
) -> impl IntoResponse {
    info!("Deleting file: {}", key);

    match client.head_object().bucket(S3_BUCKET).key(&key).send().await {
        Ok(_) => {
            match client.delete_object().bucket(S3_BUCKET).key(&key).send().await {
                Ok(_) => {
                    info!("Successfully deleted file: {}", key);
                    Ok((StatusCode::OK, "File deleted successfully").into_response())
                }
                Err(e) => {
                    error!("Failed to delete existing file {}: {}", key, e);
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to delete file '{}': {}", key, e),
                    ))
                }
            }
        }
        Err(e) => {
            if let aws_sdk_s3::error::SdkError::ServiceError(service_err) = &e {
                if service_err.err().is_not_found() {
                    error!("Attempted to delete non-existent file: {}", key);
                    return Err((StatusCode::NOT_FOUND, format!("File '{}' does not exist", key)));
                }
            }

            error!("Failed to check if file {} exists: {}", key, e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Unable to access file '{}': {}", key, e),
            ))
        }
    }
}

pub async fn list_objects(State(client): State<AppState>) -> impl IntoResponse {
    match client.list_objects_v2().bucket(S3_BUCKET).send().await {
        Ok(res) => {
            let list: Vec<String> = res
                .contents()
                .iter()
                .filter_map(|o| o.key().map(String::from))
                .collect();

            axum::Json(list).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
