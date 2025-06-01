use anyhow::{Context, Result};
use clap::Parser;
use object_store_server::{
    app::{AppBuilder, AppConfig, RepositoryBackend, StorageBackend},
    adapters::inbound::http::router::{create_router, AppState},
};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "object-store-server")]
#[command(about = "A hexagonal architecture object storage server", long_about = None)]
struct Cli {
    /// Server port to listen on
    #[arg(short, long, env = "SERVER_PORT", default_value = "3000")]
    port: u16,

    /// Server host to bind to
    #[arg(long, env = "SERVER_HOST", default_value = "0.0.0.0")]
    host: String,

    /// Storage backend type
    #[arg(long, env = "STORAGE_BACKEND", default_value = "memory")]
    storage_backend: String,

    /// Repository backend type
    #[arg(long, env = "REPOSITORY_BACKEND", default_value = "memory")]
    repository_backend: String,

    /// S3 endpoint URL (for S3/MinIO backends)
    #[arg(long, env = "S3_ENDPOINT")]
    s3_endpoint: Option<String>,

    /// S3 bucket name
    #[arg(long, env = "S3_BUCKET")]
    s3_bucket: Option<String>,

    /// S3 region
    #[arg(long, env = "S3_REGION", default_value = "us-east-1")]
    s3_region: String,

    /// S3 access key
    #[arg(long, env = "S3_ACCESS_KEY")]
    s3_access_key: Option<String>,

    /// S3 secret key
    #[arg(long, env = "S3_SECRET_KEY")]
    s3_secret_key: Option<String>,

    /// Use SSL for MinIO connection
    #[arg(long, env = "MINIO_USE_SSL", default_value = "false")]
    minio_use_ssl: bool,

    /// Database URL for repository backend (PostgreSQL)
    #[arg(long, env = "DATABASE_URL")]
    database_url: Option<String>,

    /// Log level
    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    log_level: String,
}

impl Cli {
    fn to_app_config(&self) -> Result<AppConfig> {
        let storage_backend = match self.storage_backend.as_str() {
            "memory" => StorageBackend::InMemory,
            "s3" => {
                let bucket = self.s3_bucket.clone()
                    .context("S3_BUCKET is required for S3 backend")?;
                let access_key = self.s3_access_key.clone();
                let secret_key = self.s3_secret_key.clone();
                
                StorageBackend::S3 {
                    bucket,
                    region: self.s3_region.clone(),
                    access_key,
                    secret_key,
                }
            }
            "minio" => {
                let endpoint = self.s3_endpoint.clone()
                    .context("S3_ENDPOINT is required for MinIO backend")?;
                let bucket = self.s3_bucket.clone()
                    .context("S3_BUCKET is required for MinIO backend")?;
                let access_key = self.s3_access_key.clone()
                    .context("S3_ACCESS_KEY is required for MinIO backend")?;
                let secret_key = self.s3_secret_key.clone()
                    .context("S3_SECRET_KEY is required for MinIO backend")?;
                
                StorageBackend::MinIO {
                    endpoint,
                    bucket,
                    access_key,
                    secret_key,
                    use_ssl: self.minio_use_ssl,
                }
            }
            _ => anyhow::bail!("Unknown storage backend: {}", self.storage_backend),
        };

        let repository_backend = match self.repository_backend.as_str() {
            "memory" => RepositoryBackend::InMemory,
            "database" | "db" => {
                let connection_string = self.database_url.clone()
                    .context("DATABASE_URL is required for database backend")?;
                RepositoryBackend::Database { connection_string }
            }
            _ => anyhow::bail!("Unknown repository backend: {}", self.repository_backend),
        };

        Ok(AppConfig {
            storage_backend,
            repository_backend,
        })
    }

    fn init_logging(&self) -> Result<()> {
        let env_filter = match self.log_level.to_lowercase().as_str() {
            "trace" => "trace",
            "debug" => "debug",
            "info" => "info",
            "warn" => "warn",
            "error" => "error",
            _ => "info",
        };

        tracing_subscriber::registry()
            .with(tracing_subscriber::fmt::layer())
            .init();

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env file if it exists
    dotenvy::dotenv().ok();

    // Parse CLI arguments
    let cli = Cli::parse();
    
    // Initialize logging
    cli.init_logging()?;

    info!("Starting Object Store Server");
    info!("Storage backend: {}", cli.storage_backend);
    info!("Repository backend: {}", cli.repository_backend);

    // Create app configuration
    let config = cli.to_app_config()?;

    // Build the application
    let app_builder = AppBuilder::new().with_config(config);
    let app_services = app_builder.build().await
        .context("Failed to build application")?;

    // Create the application state for the router
    let state = AppState {
        object_service: Arc::new(app_services.object_service),
        lifecycle_service: Arc::new(app_services.lifecycle_service),
        versioning_service: Arc::new(app_services.versioning_service),
    };

    // Create the router
    let router = create_router(state);

    // Bind to address
    let addr: SocketAddr = format!("{}:{}", cli.host, cli.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    
    info!("Server listening on http://{}", addr);

    // Start the server
    axum::serve(listener, router)
        .await
        .context("Failed to start server")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing() {
        let cli = Cli::parse_from(&[
            "object-store-server",
            "--port", "8080",
            "--storage-backend", "s3",
            "--s3-bucket", "test-bucket",
            "--s3-access-key", "test-key",
            "--s3-secret-key", "test-secret",
        ]);

        assert_eq!(cli.port, 8080);
        assert_eq!(cli.storage_backend, "s3");
        assert_eq!(cli.s3_bucket, Some("test-bucket".to_string()));
    }

    #[test]
    fn test_memory_config() {
        let cli = Cli::parse_from(&[
            "object-store-server",
        ]);

        let config = cli.to_app_config().unwrap();
        match config.storage_backend {
            StorageBackend::InMemory => (),
            _ => panic!("Expected InMemory backend"),
        }
    }
}