use object_store_server::{
    adapters::inbound::http::router::{AppState, create_router},
    create_in_memory_app,
};
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // Create the application services with in-memory storage
    let services = create_in_memory_app().await?;

    // Create the application state for the router
    let state = AppState {
        object_service: Arc::new(services.object_service),
        lifecycle_service: Arc::new(services.lifecycle_service),
        versioning_service: Arc::new(services.versioning_service),
    };

    // Create the router
    let app = create_router(state);

    // Set up the server address
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await?;

    println!("Object Store Server starting on http://{}", addr);
    println!("\nAvailable endpoints:");
    println!("  GET    /health                     - Health check");
    println!("  PUT    /buckets/:bucket            - Create bucket");
    println!("  DELETE /buckets/:bucket            - Delete bucket");
    println!("  GET    /buckets                    - List buckets");
    println!("  PUT    /buckets/:bucket/:key       - Upload object");
    println!("  GET    /buckets/:bucket/:key       - Get object");
    println!("  DELETE /buckets/:bucket/:key       - Delete object");
    println!("  GET    /buckets/:bucket             - List objects in bucket");
    println!("  GET    /buckets/:bucket/:key/versions - List object versions");
    println!("  PUT    /buckets/:bucket/versioning - Enable/disable versioning");
    println!("  GET    /buckets/:bucket/versioning - Get versioning status");
    println!("  PUT    /buckets/:bucket/lifecycle  - Set lifecycle rules");
    println!("  GET    /buckets/:bucket/lifecycle  - Get lifecycle rules");
    println!("  DELETE /buckets/:bucket/lifecycle  - Delete lifecycle rules");
    println!("\nExample usage with curl:");
    println!("  # Create a bucket");
    println!("  curl -X PUT http://localhost:3000/buckets/my-bucket");
    println!("  # Upload an object");
    println!("  curl -X PUT http://localhost:3000/buckets/my-bucket/hello.txt \\");
    println!("       -H 'Content-Type: text/plain' \\");
    println!("       -d 'Hello, World!'");
    println!("  # Get the object");
    println!("  curl http://localhost:3000/buckets/my-bucket/hello.txt");
    println!("  # Enable versioning");
    println!("  curl -X PUT http://localhost:3000/buckets/my-bucket/versioning \\");
    println!("       -H 'Content-Type: application/json' \\");
    println!("       -d '{{\"status\":\"Enabled\"}}'");
    println!("\nPress Ctrl+C to stop the server");

    // Run the server
    tracing::info!("Server listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
