use std::env;
use log::{ error, info };
use serde::Serialize;
use tokio_postgres::{ NoTls };

#[derive(Serialize)]
pub struct FontRecord {
  pub font_family: String,
  pub font_subfamily: String,
  pub object_path: String,
  pub checksum: String,
}

pub async fn connect_db() -> Result<tokio_postgres::Client, Box<dyn std::error::Error>> {
  info!("Connecting to database...");
  let db_url = env::var("DATABASE_URL").expect("invalid database url");

  let (client, connection) = tokio_postgres::connect(&db_url, NoTls).await?;

  tokio::spawn(async move {
    if let Err(e) = connection.await {
      error!("Database connection error: {}", e);
    }
  });
  info!("Database connection established.");

  Ok(client)
}

pub async fn insert_metadata(
  client: &tokio_postgres::Client,
  user_id: &uuid::Uuid,
  records: &Vec<FontRecord>
) -> Result<(), Box<dyn std::error::Error>> {
  if records.is_empty() {
    return Ok(());
  }

  let stmt = client.prepare(
    "INSERT INTO fonts (user_id, font_family, font_subfamily, object_path, checksum) 
     VALUES ($1, $2, $3, $4, $5)
     ON CONFLICT (user_id, object_path)
     DO UPDATE SET 
       font_family = EXCLUDED.font_family,
       font_subfamily = EXCLUDED.font_subfamily,
       checksum = EXCLUDED.checksum"
  ).await?;

  for record in records {
    client.execute(
      &stmt,
      &[
        &user_id,
        &record.font_family,
        &record.font_subfamily,
        &record.object_path,
        &record.checksum,
      ]
    ).await?;
  }

  Ok(())
}

pub async fn delete_metadata(
  client: &tokio_postgres::Client,
  user_id: &uuid::Uuid,
  object_path: &str
) -> Result<u64, Box<dyn std::error::Error>> {
  let stmt = client.prepare("DELETE FROM fonts WHERE user_id = $1 AND object_path = $2").await?;

  let row_affected = client.execute(&stmt, &[user_id, &object_path]).await?;

  Ok(row_affected)
}

pub async fn check_duplicate(
  client: &tokio_postgres::Client,
  user_id: &uuid::Uuid,
  checksum: &str,
  object_path: &str
) -> Result<Option<String>, Box<dyn std::error::Error>> {
  let query =
    "SELECT object_path FROM fonts WHERE user_id = $1 AND checksum = $2 AND object_path = $3";

  match client.query_opt(query, &[&user_id, &checksum, &object_path]).await? {
    Some(row) => Ok(Some(row.get("object_path"))),
    None => Ok(None),
  }
}

pub async fn get_metadata(
  client: &tokio_postgres::Client,
  user_id: &uuid::Uuid
) -> Result<Vec<FontRecord>, Box<dyn std::error::Error>> {
  let query =
    "SELECT font_family, font_subfamily, object_path, checksum FROM fonts WHERE user_id = $1";
  let rows = client.query(query, &[&user_id]).await?;

  let mut font_records = Vec::new();
  for row in rows {
    font_records.push(FontRecord {
      font_family: row.get("font_family"),
      font_subfamily: row.get("font_subfamily"),
      object_path: row.get("object_path"),
      checksum: row.get("checksum"),
    });
  }

  Ok(font_records)
}

// static CREATE_FONT_TABLE_SQL: &str =
//     "
//     CREATE TABLE IF NOT EXISTS fonts (
//         id SERIAL PRIMARY KEY,
//         user_id UUID NOT NULL REFERENCES auth.users(id) ON DELETE CASCADE,

//         created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
//         font_family TEXT NOT NULL,
//         font_subfamily TEXT NOT NULL,
//         object_path TEXT NOT NULL,
//         checksum TEXT NULL
//     )";

// static CREATE_TRANSACTION_TABLE_SQL: &str =
//     "
//     CREATE TABLE IF NOT EXISTS transaction (
//         id SERIAL PRIMARY KEY,
//         sync_timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
//         sync_status TEXT NOT NULL,
//         processed_count INT DEFAULT 0,
//         inserted_count INT DEFAULT 0,
//         skipped_count INT DEFAULT 0,
//         error_message TEXT NULL,
//         details JSONB NULL
//     )";

// static COPY_FONTS_SQL: &str =
//     "COPY fonts (font_family,
//     font_subfamily,
//     font_foundry,
//     font_designer,
//     font_license,
//     font_copyright,
//     file_name,
//     checksum) FROM STDIN (FORMAT BINARY)";

// pub async fn initialize_database(
//     client: &tokio_postgres::Client
// ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
//     client.simple_query(CREATE_FONT_TABLE_SQL).await?;
//     info!("Successfully font table!");

//     client.simple_query("ALTER TABLE fonts ENABLE ROW LEVEL SECURITY;").await?;
//     info!("Enabled RLS on fonts table!");

//     client.simple_query(
//         "
//         CREATE POLICY IF NOT EXISTS user_is_owner ON fonts
//         FOR ALL
//         USING (auth.uid() = user_id)
//         WITH CHECK (auth.uid() = user_id);
//         "
//     ).await?;
//     info!("Policy created for fonts table.");

//     client.simple_query(CREATE_TRANSACTION_TABLE_SQL).await?;
//     info!("Successfully transaction table!");

//     Ok(())
// }

// pub async fn insert_fonts(
//     client: &tokio_postgres::Client,
//     records: &Vec<FontRecord>
// ) -> Result<(), Box<dyn std::error::Error>> {
//     if records.is_empty() {
//         warn!("No records to insert.");
//         return Ok(());
//     }

//     let sink = client.copy_in(&COPY_FONTS_SQL[..]).await?;
//     let writer = BinaryCopyInWriter::new(
//         sink,
//         &[
//             Type::TEXT, // font_family
//             Type::TEXT, // font_subfamily
//             Type::TEXT, // font_foundry
//             Type::TEXT, // font_designer
//             Type::TEXT, // font_license
//             Type::TEXT, // font_copyright
//             Type::TEXT, // file_name
//             Type::TEXT, // checksum
//         ]
//     );
//     pin_mut!(writer);

//     for record in records {
//         writer
//             .as_mut()
//             .write(
//                 &[
//                     &record.font_family,
//                     &record.font_subfamily,
//                     &record.font_foundry,
//                     &record.font_designer,
//                     &record.font_license,
//                     &record.font_copyright,
//                     &record.file_name,
//                     &record.checksum,
//                 ]
//             ).await?;
//     }

//     writer.finish().await?;
//     println!("Inserted {} font", records.len());
//     Ok(())
// }
