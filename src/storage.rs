use std::{ env, fs::File, path::PathBuf, io::copy };
use aws_config::Region;
use aws_sdk_s3::{ config::Credentials, primitives::ByteStream, Client };
use log::info;

static S3_BUCKET: &str = "fonts";

pub async fn connect_s3() -> Result<aws_sdk_s3::Client, Box<dyn std::error::Error>> {
    info!("Connecting to S3 storage...");

    let s3_url = env::var("S3_URL").expect("Invalid s3 storage url");
    let s3_access_key = env::var("S3_ACCESS_KEY").expect("Invalid s3 storage url");
    let s3_secret = std::env::var("S3_ACCESS_KEY_SECRET").expect("S3_SECRET_KEY must be provided");
    let region = std::env::var("S3_REGION").expect("S3_REGION must be provided");

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
    path: &PathBuf,
    client: aws_sdk_s3::Client
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Uploading file: {}", path.display());

    let body = ByteStream::from_path(path).await?;
    let file_name = path.file_name().expect("Failed to extract file name").to_string_lossy();

    client.put_object().bucket(S3_BUCKET).key(file_name).body(body).send().await?;

    Ok(())
}

pub async fn download_file(
    key: &str,
    download_dir: &PathBuf,
    client: Client
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Downloading file: {}", key);

    let resp = client.get_object().bucket(S3_BUCKET).key(key).send().await?;
    let file_path = download_dir.join(key);

    let mut file = File::create(&file_path)?;
    <dyn tokio::io::AsyncRead>::copy(&mut resp.body.into_async_read(), &mut file).await?;

    info!("File downloaded to: {}", file_path.display());

    Ok(())
}
