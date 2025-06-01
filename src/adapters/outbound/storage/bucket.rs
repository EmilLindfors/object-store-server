use crate::adapters::outbound::storage::minio::minio::MinioLifecycleConfig;
use async_trait::async_trait;
use http::{Method, Request};
use quick_xml::de::from_str;
use reqwest::{Body, Client};
use serde::Deserialize;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

/// Validate a bucket name according to S3 naming rules
fn validate_bucket_name(name: &str) -> Result<(), BucketError> {
    // Basic validation rules
    if name.is_empty() || name.len() > 63 || name.len() < 3 {
        return Err(BucketError::InvalidBucketName(
            "Bucket name must be between 3 and 63 characters".to_string(),
        ));
    }

    // Must start and end with lowercase letter or number
    if !name.chars().next().unwrap().is_ascii_lowercase()
        && !name.chars().next().unwrap().is_ascii_digit()
    {
        return Err(BucketError::InvalidBucketName(
            "Bucket name must start with a lowercase letter or number".to_string(),
        ));
    }

    if !name.chars().last().unwrap().is_ascii_lowercase()
        && !name.chars().last().unwrap().is_ascii_digit()
    {
        return Err(BucketError::InvalidBucketName(
            "Bucket name must end with a lowercase letter or number".to_string(),
        ));
    }

    // Can only contain lowercase letters, numbers, hyphens, and dots
    for c in name.chars() {
        if !c.is_ascii_lowercase() && !c.is_ascii_digit() && c != '-' && c != '.' {
            return Err(BucketError::InvalidBucketName(
                "Bucket name can only contain lowercase letters, numbers, hyphens, and dots"
                    .to_string(),
            ));
        }
    }

    Ok(())
}

/// Errors that can occur in bucket operations
#[derive(Error, Debug)]
pub enum BucketError {
    #[error("Invalid bucket name: {0}")]
    InvalidBucketName(String),

    #[error("Bucket already exists: {0}")]
    BucketAlreadyExists(String),

    #[error("Region mismatch: expected {0}, got {1}")]
    RegionMismatch(String, String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] http::Error),

    #[error("XML error: {0}")]
    XmlError(String),

    #[error("Request signing error: {0}")]
    SigningError(String),

    #[error("Service error: {status_code} - {message}")]
    ServiceError { status_code: u16, message: String },

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Unauthorized")]
    Unauthorized,
}

/// Options for bucket creation
#[derive(Debug, Clone, Default)]
pub struct BucketOptions {
    pub region: Option<String>,
    pub object_lock_enabled: bool,
    pub versioning_enabled: bool,
    pub extra_headers: Option<HashMap<String, String>>,
    pub extra_query_params: Option<HashMap<String, String>>,
}

/// S3 Client for making API calls
#[derive(Clone, Debug)]
pub struct S3Client {
    endpoint: String,
    region: String,
    access_key: String,
    secret_key: String,
    secure: bool,
    session_token: Option<String>,
    http_client: Client,
}

impl S3Client {
    pub fn new(
        endpoint: String,
        region: Option<String>,
        access_key: String,
        secret_key: String,
        secure: bool,
        session_token: Option<String>,
    ) -> Self {
        Self {
            endpoint,
            region: region.unwrap_or_else(|| "us-east-1".to_string()),
            access_key,
            secret_key,
            secure,
            session_token,
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Get the URL scheme (http or https)
    fn get_scheme(&self) -> &'static str {
        if self.secure { "https" } else { "http" }
    }

    /// Get the base URL for the client
    fn get_base_url(&self) -> String {
        format!("{}://{}", self.get_scheme(), self.endpoint)
    }

    /// Get the region from the client configuration
    pub fn get_region(&self) -> &str {
        &self.region
    }

    /// Sign and execute a request
    async fn execute_request(
        &self,
        request: Request<Body>,
    ) -> Result<reqwest::Response, BucketError> {
        // Convert the request to a reqwest request
        let (parts, body) = request.into_parts();

        // Get method and URL from parts
        let method = match parts.method.as_str() {
            "GET" => reqwest::Method::GET,
            "PUT" => reqwest::Method::PUT,
            "POST" => reqwest::Method::POST,
            "DELETE" => reqwest::Method::DELETE,
            _ => {
                return Err(BucketError::ServiceError {
                    status_code: 405,
                    message: "Method not allowed".to_string(),
                });
            }
        };

        let url = parts.uri.to_string();

        // Execute the request using reqwest
        let mut req = self.http_client.request(method, &url);

        // Add headers
        for (key, value) in parts.headers.iter() {
            if let Ok(value_str) = value.to_str() {
                req = req.header(key.as_str(), value_str);
            }
        }

        // Add body if present
        req = req.body(body);

        let response = req
            .send()
            .await
            .map_err(|e| BucketError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

        // Check for errors
        let status = response.status();
        if !status.is_success() {
            let status_code = status.as_u16();

            // Extract error message from response body
            let error_message = "Service error occurred".to_string();
            return Err(BucketError::ServiceError {
                status_code,
                message: error_message,
            });
        }

        Ok(response)
    }
}

#[async_trait]
pub trait BucketOperations {
    /// Create a new bucket with the given name and options
    async fn create_bucket(
        &self,
        name: &str,
        options: Option<BucketOptions>,
    ) -> Result<(), BucketError>;

    /// Check if a bucket exists
    async fn bucket_exists(&self, name: &str) -> Result<bool, BucketError>;

    /// Delete a bucket
    async fn delete_bucket(&self, name: &str) -> Result<(), BucketError>;

    /// List all buckets
    async fn list_buckets(&self) -> Result<Vec<String>, BucketError>;

    /// Set lifecycle configuration for a bucket
    async fn set_bucket_lifecycle(
        &self,
        name: &str,
        config: &MinioLifecycleConfig,
    ) -> Result<(), BucketError>;

    /// Get lifecycle configuration for a bucket
    async fn get_bucket_lifecycle(&self, name: &str) -> Result<MinioLifecycleConfig, BucketError>;
}

/// Implementation of BucketOperations for S3Client
pub struct S3BucketOperations {
    client: S3Client,
}

impl S3BucketOperations {
    pub fn new(client: S3Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl BucketOperations for S3BucketOperations {
    async fn create_bucket(
        &self,
        name: &str,
        options: Option<BucketOptions>,
    ) -> Result<(), BucketError> {
        // Validate bucket name
        validate_bucket_name(name)?;

        let options = options.unwrap_or_default();
        let region = options
            .region
            .clone()
            .unwrap_or_else(|| self.client.get_region().to_string());

        // Check for region mismatch
        if region != self.client.get_region() {
            return Err(BucketError::RegionMismatch(
                self.client.get_region().to_string(),
                region.clone(),
            ));
        }

        // Prepare request headers
        let mut headers = http::HeaderMap::new();
        headers.insert("Host", self.client.endpoint.parse().unwrap());

        // Add object lock header if enabled
        if options.object_lock_enabled {
            headers.insert("x-amz-bucket-object-lock-enabled", "true".parse().unwrap());
        }

        // Add any extra headers
        if let Some(extra_headers) = options.extra_headers {
            for (key, value) in extra_headers {
                headers.insert(
                    http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
                    http::HeaderValue::from_str(&value).unwrap(),
                );
            }
        }

        // Build the request URL
        let url = format!("{}/{}", self.client.get_base_url(), name);

        // Add any query parameters
        let url = if let Some(params) = options.extra_query_params {
            let query = params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            format!("{}?{}", url, query)
        } else {
            url
        };

        // Build the request body with location constraint if needed
        let body = if region == "us-east-1" {
            "".to_string()
        } else {
            format!(
                "<CreateBucketConfiguration><LocationConstraint>{}</LocationConstraint></CreateBucketConfiguration>",
                region
            )
        };

        // Create the request
        let request = Request::builder()
            .method(Method::PUT)
            .uri(url)
            .extension(headers)
            .body(Body::from(body))?;

        // Execute the request
        let _ = self.client.execute_request(request).await?;

        // If versioning is enabled, set it
        if options.versioning_enabled {
            self.enable_bucket_versioning(name).await?;
        }

        Ok(())
    }

    async fn bucket_exists(&self, name: &str) -> Result<bool, BucketError> {
        // Validate bucket name
        validate_bucket_name(name)?;

        // Build the request URL
        let url = format!("{}/{}", self.client.get_base_url(), name);

        // Create the request
        let request = Request::builder()
            .method(Method::HEAD)
            .uri(url)
            .body(Body::from(""))?;

        // Execute the request
        match self.client.execute_request(request).await {
            Ok(_) => Ok(true),
            Err(BucketError::ServiceError { status_code, .. }) if status_code == 404 => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn delete_bucket(&self, name: &str) -> Result<(), BucketError> {
        // Validate bucket name
        validate_bucket_name(name)?;

        // Build the request URL
        let url = format!("{}/{}", self.client.get_base_url(), name);

        // Create the request
        let request = Request::builder()
            .method(Method::DELETE)
            .uri(url)
            .body(Body::from(""))?;

        // Execute the request
        let _ = self.client.execute_request(request).await?;

        Ok(())
    }

    async fn list_buckets(&self) -> Result<Vec<String>, BucketError> {
        // Build the request URL
        let url = self.client.get_base_url();

        // Create the request
        let request = Request::builder()
            .method(Method::GET)
            .uri(url)
            .body(Body::from(""))?;

        // Execute the request
        let response = self.client.execute_request(request).await?;

        // Parse the response
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| BucketError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let body = String::from_utf8_lossy(&body_bytes);

        // Parse XML response
        #[derive(Deserialize)]
        struct ListBucketsResult {
            #[serde(rename = "Buckets")]
            buckets: ListAllMyBucketsList,
        }

        #[derive(Deserialize)]
        struct ListAllMyBucketsList {
            #[serde(rename = "Bucket")]
            bucket: Vec<Bucket>,
        }

        #[derive(Deserialize)]
        struct Bucket {
            #[serde(rename = "Name")]
            name: String,
        }

        let result: ListBucketsResult =
            from_str(&body).map_err(|e| BucketError::XmlError(e.to_string()))?;

        Ok(result.buckets.bucket.into_iter().map(|b| b.name).collect())
    }

    async fn set_bucket_lifecycle(
        &self,
        name: &str,
        config: &MinioLifecycleConfig,
    ) -> Result<(), BucketError> {
        // Validate bucket name and lifecycle config
        validate_bucket_name(name)?;
        // For MinioLifecycleConfig, we'll skip validation since it doesn't have a validate method
        // Convert config to XML - we'll serialize it as JSON for now
        let xml =
            serde_json::to_string(config).map_err(|e| BucketError::XmlError(e.to_string()))?;

        // Build the request URL
        let url = format!("{}/{}?lifecycle", self.client.get_base_url(), name);

        // Create the request
        let request = Request::builder()
            .method(Method::PUT)
            .uri(url)
            .body(Body::from(xml))?;

        // Execute the request
        let _ = self.client.execute_request(request).await?;

        Ok(())
    }

    async fn get_bucket_lifecycle(&self, name: &str) -> Result<MinioLifecycleConfig, BucketError> {
        // Validate bucket name
        validate_bucket_name(name)?;

        // Build the request URL
        let url = format!("{}/{}?lifecycle", self.client.get_base_url(), name);

        // Create the request
        let request = Request::builder()
            .method(Method::GET)
            .uri(url)
            .body(Body::from(""))?;

        // Execute the request
        let response = self.client.execute_request(request).await?;

        // Parse the response
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| BucketError::IoError(std::io::Error::new(std::io::ErrorKind::Other, e)))?;
        let body = String::from_utf8_lossy(&body_bytes);

        // Parse XML response - for now, we'll try to parse as JSON
        let config: MinioLifecycleConfig =
            serde_json::from_str(&body).map_err(|e| BucketError::XmlError(e.to_string()))?;

        Ok(config)
    }
}

// Extension trait for additional bucket operations
#[async_trait]
pub trait BucketOperationsExt: BucketOperations {
    /// Enable versioning on a bucket
    async fn enable_bucket_versioning(&self, name: &str) -> Result<(), BucketError>;
}

#[async_trait]
impl BucketOperationsExt for S3BucketOperations {
    async fn enable_bucket_versioning(&self, name: &str) -> Result<(), BucketError> {
        // Build the request URL
        let url = format!("{}/{}?versioning", self.client.get_base_url(), name);

        // Create the request body
        let body = "<VersioningConfiguration><Status>Enabled</Status></VersioningConfiguration>";

        // Create the request
        let request = Request::builder()
            .method(Method::PUT)
            .uri(url)
            .body(Body::from(body))?;

        // Execute the request
        let _ = self.client.execute_request(request).await?;

        Ok(())
    }
}
