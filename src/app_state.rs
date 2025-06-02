use std::sync::Arc;

use crate::{ database::connect_db, storage::connect_s3 };
pub type AppState = Arc<AppStateInner>;

pub struct AppStateInner {
    pub s3_client: aws_sdk_s3::Client,
    pub db_client: tokio_postgres::Client,
}

pub async fn create_app_state() -> Result<Arc<AppStateInner>, Box<dyn std::error::Error>> {
    let s3_client = connect_s3().await?;
    let db_client = connect_db().await?;

    Ok(
        Arc::new(AppStateInner {
            s3_client,
            db_client,
        })
    )
}
