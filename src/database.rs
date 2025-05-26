use std::env;

use log::{ error, info };
use tokio_postgres::{ binary_copy::BinaryCopyInWriter, types::Type, NoTls };
use futures::pin_mut;

struct FontRecord {
    pub font_family: String,
    pub font_subfamily: String,
    pub font_designer: String,
    pub font_foundry: String,
    pub font_license: String,
    pub font_copyright: String,
    pub file_name: String,
    pub file_path: String,
    pub checksum: String,
}

pub async fn connect_db() -> Result<tokio_postgres::Client, Box<dyn std::error::Error>> {
    info!("Connecting to database...");
    let db_url = env::var("POOL_DATABASE_URL").expect("invalid database url");

    let (client, connection) = tokio_postgres::connect(&db_url, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            error!("Database connection error: {}", e);
        }
    });
    info!("Database connection established.");

    Ok(client)
}

pub async fn initialize_database(
    client: &tokio_postgres::Client
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let fonts_table =
        "
        CREATE TABLE IF NOT EXISTS fonts (
            id SERIAL PRIMARY KEY,
            created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            font_family TEXT,
            font_subfamily TEXT,
            font_foundry TEXT,
            font_designer TEXT,
            font_license TEXT,
            font_copyright TEXT,
            file_name TEXT,
            file_url TEXT NULL,
            checksum TEXT
        )
        ";

    client.simple_query(&fonts_table).await?;
    info!("Successfully font table!");

    let transaction_table =
        "        
        CREATE TABLE IF NOT EXISTS transaction (
            id SERIAL PRIMARY KEY,
            sync_timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            sync_status TEXT NOT NULL,
            processed_count INT DEFAULT 0,
            inserted_count INT DEFAULT 0,
            skipped_count INT DEFAULT 0,
            error_message TEXT NULL,
            details JSONB NULL
        )";

    client.simple_query(&transaction_table).await?;
    info!("Successfully transaction table!");

    Ok(())
}

pub async fn insert_fonts(
    client: &tokio_postgres::Client,
    records: &Vec<FontRecord>
) -> Result<(), Box<dyn std::error::Error>> {
    if records.is_empty() {
        println!("No records to insert.");
        return Ok(());
    }

    let copy_sql = format!(
        "COPY fonts (font_family, font_subfamily, font_foundry, font_designer, font_license, font_copyright, file_name, checksum) FROM STDIN (FORMAT BINARY)"
    );

    let sink = client.copy_in(&copy_sql[..]).await?;
    let writer = BinaryCopyInWriter::new(
        sink,
        &[
            Type::TEXT, // font_family
            Type::TEXT, // font_subfamily
            Type::TEXT, // font_foundry
            Type::TEXT, // font_designer
            Type::TEXT, // font_license
            Type::TEXT, // font_copyright
            Type::TEXT, // file_name
            Type::TEXT, // checksum
        ]
    );
    pin_mut!(writer);

    for record in records {
        writer
            .as_mut()
            .write(
                &[
                    &record.font_family,
                    &record.font_subfamily,
                    &record.font_foundry,
                    &record.font_designer,
                    &record.font_license,
                    &record.font_copyright,
                    &record.file_name,
                    &record.checksum,
                ]
            ).await?;
    }

    writer.finish().await?;
    println!("Inserted {} font", records.len());
    Ok(())
}
