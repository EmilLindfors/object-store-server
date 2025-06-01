use crate::adapters::outbound::storage::error::StoreError;
use chrono::{DateTime, Utc};
use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use reqwest::Client;
use std::borrow::Cow;
use std::io::Cursor;
use std::time::Duration;

/// MinIO-specific lifecycle configuration
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinioLifecycleConfig {
    pub rules: Vec<MinioLifecycleRule>,
}

/// MinIO-specific lifecycle rule
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinioLifecycleRule {
    pub id: String,
    pub status: bool,
    pub filter: MinioFilter,
    pub abort_incomplete_multipart_upload_days_after_initiation: Option<usize>,
    pub expiration_date: Option<DateTime<Utc>>,
    pub expiration_days: Option<usize>,
    pub expiration_expired_object_delete_marker: Option<bool>,
    pub noncurrent_version_expiration_noncurrent_days: Option<usize>,
    pub noncurrent_version_transition_noncurrent_days: Option<usize>,
    pub noncurrent_version_transition_storage_class: Option<String>,
    pub transition_date: Option<DateTime<Utc>>,
    pub transition_days: Option<usize>,
    pub transition_storage_class: Option<String>,
}

/// MinIO-specific filter
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinioFilter {
    pub prefix: Option<String>,
    pub tag: Option<String>,
    pub and: Option<String>,
}

/// Client for interacting with MinIO specific APIs
pub struct MinioClient {
    client: Client,
    endpoint: String,
    access_key: String,
    secret_key: String,
    region: String,
}

impl MinioClient {
    /// Create a new MinIO client
    pub fn new(endpoint: &str, access_key: &str, secret_key: &str, region: &str) -> Self {
        // Create reqwest client with reasonable defaults
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        MinioClient {
            client,
            endpoint: endpoint.to_string(),
            access_key: access_key.to_string(),
            secret_key: secret_key.to_string(),
            region: region.to_string(),
        }
    }

    /// Create a client from existing AWS S3 credentials for convenience
    pub fn from_s3_config(
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
        region: &str,
    ) -> Self {
        Self::new(endpoint, access_key, secret_key, region)
    }

    /// Get the lifecycle configuration for a bucket
    pub async fn get_lifecycle_config(
        &self,
        bucket: &str,
    ) -> Result<MinioLifecycleConfig, StoreError> {
        let url = format!("{}/{}", self.endpoint, bucket);

        // Add the querystring to request lifecycle configuration
        let url = format!("{}?lifecycle", url);

        let response = self
            .client
            .get(&url)
            .basic_auth(&self.access_key, Some(&self.secret_key))
            .send()
            .await
            .map_err(|e| {
                StoreError::Other(format!("Failed to get lifecycle configuration: {}", e))
            })?;

        if !response.status().is_success() {
            // If not found, return empty config
            if response.status().as_u16() == 404 {
                return Ok(MinioLifecycleConfig::default());
            }

            return Err(StoreError::Other(format!(
                "Failed to get lifecycle configuration: {}",
                response.status()
            )));
        }

        // Get response body as text
        let xml = response.text().await.map_err(|e| {
            StoreError::Other(format!("Failed to read lifecycle configuration: {}", e))
        })?;

        // Parse XML response
        let config = parse_lifecycle_config(&xml)?;

        Ok(config)
    }

    /// Set the lifecycle configuration for a bucket
    pub async fn set_lifecycle_config(
        &self,
        bucket: &str,
        config: &MinioLifecycleConfig,
    ) -> Result<(), StoreError> {
        let url = format!("{}/{}", self.endpoint, bucket);

        // Add the querystring to set lifecycle configuration
        let url = format!("{}?lifecycle", url);

        // Convert config to XML
        let xml = lifecycle_config_to_xml(config)?;

        let response = self
            .client
            .put(&url)
            .basic_auth(&self.access_key, Some(&self.secret_key))
            .header("Content-Type", "application/xml")
            .body(xml)
            .send()
            .await
            .map_err(|e| {
                StoreError::Other(format!("Failed to set lifecycle configuration: {}", e))
            })?;

        let response_status = response.status();

        if !response_status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(StoreError::Other(format!(
                "Failed to set lifecycle configuration: {} - {}",
                response_status, error_text
            )));
        }

        Ok(())
    }

    /// Delete the lifecycle configuration for a bucket
    pub async fn delete_lifecycle_config(&self, bucket: &str) -> Result<(), StoreError> {
        let url = format!("{}/{}", self.endpoint, bucket);

        // Add the querystring to delete lifecycle configuration
        let url = format!("{}?lifecycle", url);

        let response = self
            .client
            .delete(&url)
            .basic_auth(&self.access_key, Some(&self.secret_key))
            .send()
            .await
            .map_err(|e| {
                StoreError::Other(format!("Failed to delete lifecycle configuration: {}", e))
            })?;

        if !response.status().is_success() {
            return Err(StoreError::Other(format!(
                "Failed to delete lifecycle configuration: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Parse a date string in ISO 8601 format to a DateTime<Utc>
fn parse_iso8601_date(date_str: &str) -> Result<DateTime<Utc>, StoreError> {
    match DateTime::parse_from_rfc3339(date_str) {
        Ok(dt) => Ok(dt.with_timezone(&Utc)),
        Err(e) => Err(StoreError::Other(format!("Failed to parse date: {}", e))),
    }
}

/// Format a DateTime<Utc> to an ISO 8601 string
fn format_iso8601_date(date: DateTime<Utc>) -> String {
    date.to_rfc3339()
}

// Helper function to get text content of an XML element
fn get_element_text<'a, T: std::io::BufRead>(
    reader: &mut quick_xml::Reader<T>,
    buf: &'a mut Vec<u8>,
) -> Result<Cow<'a, str>, StoreError> {
    match reader.read_event_into(buf) {
        Ok(Event::Text(e)) => {
            let text = e
                .unescape()
                .map_err(|e| StoreError::Other(format!("Failed to unescape XML text: {}", e)))?;
            Ok(text)
        }
        _ => Err(StoreError::Other("Expected text content".to_string())),
    }
}

// Helper function to parse XML to MinioLifecycleConfig
fn parse_lifecycle_config(xml: &str) -> Result<MinioLifecycleConfig, StoreError> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.trim_text(true);

    let mut config = MinioLifecycleConfig::default();
    let mut buf = Vec::new();

    // Current rule being parsed and state tracking
    let mut current_rule: Option<MinioLifecycleRule> = None;
    let mut in_expiration = false;
    let mut in_filter = false;
    let mut in_transition = false;
    let mut in_noncurrent_version_expiration = false;
    let mut in_noncurrent_version_transition = false;
    let _current_tag_key: Option<String> = None;

    // Parse the XML
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name().as_ref() {
                    b"Rule" => {
                        // Start a new rule with default values
                        current_rule = Some(MinioLifecycleRule {
                            id: String::new(),
                            status: false,
                            filter: MinioFilter::default(),
                            abort_incomplete_multipart_upload_days_after_initiation: None,
                            expiration_date: None,
                            expiration_days: None,
                            expiration_expired_object_delete_marker: None,
                            noncurrent_version_expiration_noncurrent_days: None,
                            noncurrent_version_transition_noncurrent_days: None,
                            noncurrent_version_transition_storage_class: None,
                            transition_date: None,
                            transition_days: None,
                            transition_storage_class: None,
                        });
                    }
                    b"Expiration" => {
                        in_expiration = true;
                    }
                    b"Filter" => {
                        in_filter = true;
                    }
                    b"Transition" => {
                        in_transition = true;
                    }
                    b"NoncurrentVersionExpiration" => {
                        in_noncurrent_version_expiration = true;
                    }
                    b"NoncurrentVersionTransition" => {
                        in_noncurrent_version_transition = true;
                    }
                    b"ID" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            rule.id = text.to_string();
                        }
                    }
                    b"Status" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            rule.status = text.to_lowercase() == "enabled";
                        }
                    }
                    b"Days" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            let days = text.parse::<usize>().map_err(|e| {
                                StoreError::Other(format!("Failed to parse days: {}", e))
                            })?;

                            if in_expiration {
                                rule.expiration_days = Some(days);
                            } else if in_transition {
                                rule.transition_days = Some(days);
                            }
                        }
                    }
                    b"Date" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            let date = parse_iso8601_date(&text)?;

                            if in_expiration {
                                rule.expiration_date = Some(date);
                            } else if in_transition {
                                rule.transition_date = Some(date);
                            }
                        }
                    }
                    b"StorageClass" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;

                            if in_transition {
                                rule.transition_storage_class = Some(text.to_string());
                            } else if in_noncurrent_version_transition {
                                rule.noncurrent_version_transition_storage_class =
                                    Some(text.to_string());
                            }
                        }
                    }
                    b"NoncurrentDays" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            let days = text.parse::<usize>().map_err(|e| {
                                StoreError::Other(format!("Failed to parse noncurrent days: {}", e))
                            })?;

                            if in_noncurrent_version_expiration {
                                rule.noncurrent_version_expiration_noncurrent_days = Some(days);
                            } else if in_noncurrent_version_transition {
                                rule.noncurrent_version_transition_noncurrent_days = Some(days);
                            }
                        }
                    }
                    b"DaysAfterInitiation" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            let days = text.parse::<usize>().map_err(|e| {
                                StoreError::Other(format!(
                                    "Failed to parse days after initiation: {}",
                                    e
                                ))
                            })?;
                            rule.abort_incomplete_multipart_upload_days_after_initiation =
                                Some(days);
                        }
                    }
                    b"ExpiredObjectDeleteMarker" => {
                        if let Some(ref mut rule) = current_rule {
                            buf.clear();
                            let text = get_element_text(&mut reader, &mut buf)?;
                            rule.expiration_expired_object_delete_marker =
                                Some(text.to_lowercase() == "true");
                        }
                    }
                    b"Prefix" => {
                        if let Some(ref mut rule) = current_rule {
                            if in_filter {
                                buf.clear();
                                let text = get_element_text(&mut reader, &mut buf)?;
                                rule.filter.prefix = Some(text.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                match e.name().as_ref() {
                    b"Rule" => {
                        // End of rule, add to config if valid
                        if let Some(rule) = current_rule.take() {
                            config.rules.push(rule);
                        }
                    }
                    b"Expiration" => {
                        in_expiration = false;
                    }
                    b"Filter" => {
                        in_filter = false;
                    }
                    b"Transition" => {
                        in_transition = false;
                    }
                    b"NoncurrentVersionExpiration" => {
                        in_noncurrent_version_expiration = false;
                    }
                    b"NoncurrentVersionTransition" => {
                        in_noncurrent_version_transition = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                // Handle self-closing tags
                match e.name().as_ref() {
                    b"ExpiredObjectDeleteMarker" => {
                        if let Some(ref mut rule) = current_rule {
                            rule.expiration_expired_object_delete_marker = Some(true);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(StoreError::Other(format!("Error parsing XML: {}", e)));
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(config)
}

// Helper function to convert MinioLifecycleConfig to XML
fn lifecycle_config_to_xml(config: &MinioLifecycleConfig) -> Result<String, StoreError> {
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // Write XML declaration
    writer
        .write_event(Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            None,
        )))
        .map_err(|e| StoreError::Other(format!("Failed to write XML declaration: {}", e)))?;

    // Start LifecycleConfiguration
    writer
        .write_event(Event::Start(BytesStart::new("LifecycleConfiguration")))
        .map_err(|e| {
            StoreError::Other(format!(
                "Failed to write LifecycleConfiguration start: {}",
                e
            ))
        })?;

    // Write each rule
    for rule in &config.rules {
        // Start Rule
        writer
            .write_event(Event::Start(BytesStart::new("Rule")))
            .map_err(|e| StoreError::Other(format!("Failed to write Rule start: {}", e)))?;

        // Write ID if not empty
        if !rule.id.is_empty() {
            writer
                .write_event(Event::Start(BytesStart::new("ID")))
                .map_err(|e| StoreError::Other(format!("Failed to write ID start: {}", e)))?;
            writer
                .write_event(Event::Text(BytesText::new(&rule.id)))
                .map_err(|e| StoreError::Other(format!("Failed to write ID text: {}", e)))?;
            writer
                .write_event(Event::End(BytesEnd::new("ID")))
                .map_err(|e| StoreError::Other(format!("Failed to write ID end: {}", e)))?;
        }

        // Write Status
        writer
            .write_event(Event::Start(BytesStart::new("Status")))
            .map_err(|e| StoreError::Other(format!("Failed to write Status start: {}", e)))?;
        writer
            .write_event(Event::Text(BytesText::new(if rule.status {
                "Enabled"
            } else {
                "Disabled"
            })))
            .map_err(|e| StoreError::Other(format!("Failed to write Status text: {}", e)))?;
        writer
            .write_event(Event::End(BytesEnd::new("Status")))
            .map_err(|e| StoreError::Other(format!("Failed to write Status end: {}", e)))?;

        // Write Filter
        writer
            .write_event(Event::Start(BytesStart::new("Filter")))
            .map_err(|e| StoreError::Other(format!("Failed to write Filter start: {}", e)))?;

        // Write Prefix if exists
        if let Some(ref prefix) = rule.filter.prefix {
            writer
                .write_event(Event::Start(BytesStart::new("Prefix")))
                .map_err(|e| StoreError::Other(format!("Failed to write Prefix start: {}", e)))?;
            writer
                .write_event(Event::Text(BytesText::new(prefix)))
                .map_err(|e| StoreError::Other(format!("Failed to write Prefix text: {}", e)))?;
            writer
                .write_event(Event::End(BytesEnd::new("Prefix")))
                .map_err(|e| StoreError::Other(format!("Failed to write Prefix end: {}", e)))?;
        }

        // End Filter
        writer
            .write_event(Event::End(BytesEnd::new("Filter")))
            .map_err(|e| StoreError::Other(format!("Failed to write Filter end: {}", e)))?;

        // Write Expiration if needed
        if rule.expiration_date.is_some()
            || rule.expiration_days.is_some()
            || rule.expiration_expired_object_delete_marker.is_some()
        {
            writer
                .write_event(Event::Start(BytesStart::new("Expiration")))
                .map_err(|e| {
                    StoreError::Other(format!("Failed to write Expiration start: {}", e))
                })?;

            // Write Days if exists
            if let Some(days) = rule.expiration_days {
                writer
                    .write_event(Event::Start(BytesStart::new("Days")))
                    .map_err(|e| StoreError::Other(format!("Failed to write Days start: {}", e)))?;
                writer
                    .write_event(Event::Text(BytesText::new(&days.to_string())))
                    .map_err(|e| StoreError::Other(format!("Failed to write Days text: {}", e)))?;
                writer
                    .write_event(Event::End(BytesEnd::new("Days")))
                    .map_err(|e| StoreError::Other(format!("Failed to write Days end: {}", e)))?;
            }

            // Write Date if exists
            if let Some(date) = rule.expiration_date {
                writer
                    .write_event(Event::Start(BytesStart::new("Date")))
                    .map_err(|e| StoreError::Other(format!("Failed to write Date start: {}", e)))?;
                writer
                    .write_event(Event::Text(BytesText::new(&format_iso8601_date(date))))
                    .map_err(|e| StoreError::Other(format!("Failed to write Date text: {}", e)))?;
                writer
                    .write_event(Event::End(BytesEnd::new("Date")))
                    .map_err(|e| StoreError::Other(format!("Failed to write Date end: {}", e)))?;
            }

            // Write ExpiredObjectDeleteMarker if exists
            if let Some(marker) = rule.expiration_expired_object_delete_marker {
                writer
                    .write_event(Event::Start(BytesStart::new("ExpiredObjectDeleteMarker")))
                    .map_err(|e| {
                        StoreError::Other(format!(
                            "Failed to write ExpiredObjectDeleteMarker start: {}",
                            e
                        ))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(&marker.to_string())))
                    .map_err(|e| {
                        StoreError::Other(format!(
                            "Failed to write ExpiredObjectDeleteMarker text: {}",
                            e
                        ))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("ExpiredObjectDeleteMarker")))
                    .map_err(|e| {
                        StoreError::Other(format!(
                            "Failed to write ExpiredObjectDeleteMarker end: {}",
                            e
                        ))
                    })?;
            }

            // End Expiration
            writer
                .write_event(Event::End(BytesEnd::new("Expiration")))
                .map_err(|e| StoreError::Other(format!("Failed to write Expiration end: {}", e)))?;
        }

        // Write Transition if needed
        if rule.transition_date.is_some()
            || rule.transition_days.is_some()
            || rule.transition_storage_class.is_some()
        {
            writer
                .write_event(Event::Start(BytesStart::new("Transition")))
                .map_err(|e| {
                    StoreError::Other(format!("Failed to write Transition start: {}", e))
                })?;

            // Write Days if exists
            if let Some(days) = rule.transition_days {
                writer
                    .write_event(Event::Start(BytesStart::new("Days")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write Transition Days start: {}", e))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(&days.to_string())))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write Transition Days text: {}", e))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("Days")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write Transition Days end: {}", e))
                    })?;
            }

            // Write Date if exists
            if let Some(date) = rule.transition_date {
                writer
                    .write_event(Event::Start(BytesStart::new("Date")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write Transition Date start: {}", e))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(&format_iso8601_date(date))))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write Transition Date text: {}", e))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("Date")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write Transition Date end: {}", e))
                    })?;
            }

            // Write StorageClass if exists
            if let Some(ref storage_class) = rule.transition_storage_class {
                writer
                    .write_event(Event::Start(BytesStart::new("StorageClass")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write StorageClass start: {}", e))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(storage_class)))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write StorageClass text: {}", e))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("StorageClass")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write StorageClass end: {}", e))
                    })?;
            }

            // End Transition
            writer
                .write_event(Event::End(BytesEnd::new("Transition")))
                .map_err(|e| StoreError::Other(format!("Failed to write Transition end: {}", e)))?;
        }

        // Write NoncurrentVersionExpiration if needed
        if rule.noncurrent_version_expiration_noncurrent_days.is_some() {
            writer
                .write_event(Event::Start(BytesStart::new("NoncurrentVersionExpiration")))
                .map_err(|e| {
                    StoreError::Other(format!(
                        "Failed to write NoncurrentVersionExpiration start: {}",
                        e
                    ))
                })?;

            // Write NoncurrentDays
            if let Some(days) = rule.noncurrent_version_expiration_noncurrent_days {
                writer
                    .write_event(Event::Start(BytesStart::new("NoncurrentDays")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write NoncurrentDays start: {}", e))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(&days.to_string())))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write NoncurrentDays text: {}", e))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("NoncurrentDays")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write NoncurrentDays end: {}", e))
                    })?;
            }

            // End NoncurrentVersionExpiration
            writer
                .write_event(Event::End(BytesEnd::new("NoncurrentVersionExpiration")))
                .map_err(|e| {
                    StoreError::Other(format!(
                        "Failed to write NoncurrentVersionExpiration end: {}",
                        e
                    ))
                })?;
        }

        // Write NoncurrentVersionTransition if needed
        if rule.noncurrent_version_transition_noncurrent_days.is_some()
            || rule.noncurrent_version_transition_storage_class.is_some()
        {
            writer
                .write_event(Event::Start(BytesStart::new("NoncurrentVersionTransition")))
                .map_err(|e| {
                    StoreError::Other(format!(
                        "Failed to write NoncurrentVersionTransition start: {}",
                        e
                    ))
                })?;

            // Write NoncurrentDays
            if let Some(days) = rule.noncurrent_version_transition_noncurrent_days {
                writer
                    .write_event(Event::Start(BytesStart::new("NoncurrentDays")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write NoncurrentDays start: {}", e))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(&days.to_string())))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write NoncurrentDays text: {}", e))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("NoncurrentDays")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write NoncurrentDays end: {}", e))
                    })?;
            }

            // Write StorageClass
            if let Some(ref storage_class) = rule.noncurrent_version_transition_storage_class {
                writer
                    .write_event(Event::Start(BytesStart::new("StorageClass")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write StorageClass start: {}", e))
                    })?;
                writer
                    .write_event(Event::Text(BytesText::new(storage_class)))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write StorageClass text: {}", e))
                    })?;
                writer
                    .write_event(Event::End(BytesEnd::new("StorageClass")))
                    .map_err(|e| {
                        StoreError::Other(format!("Failed to write StorageClass end: {}", e))
                    })?;
            }

            // End NoncurrentVersionTransition
            writer
                .write_event(Event::End(BytesEnd::new("NoncurrentVersionTransition")))
                .map_err(|e| {
                    StoreError::Other(format!(
                        "Failed to write NoncurrentVersionTransition end: {}",
                        e
                    ))
                })?;
        }

        // Write AbortIncompleteMultipartUpload if needed
        if let Some(days) = rule.abort_incomplete_multipart_upload_days_after_initiation {
            writer
                .write_event(Event::Start(BytesStart::new(
                    "AbortIncompleteMultipartUpload",
                )))
                .map_err(|e| {
                    StoreError::Other(format!(
                        "Failed to write AbortIncompleteMultipartUpload start: {}",
                        e
                    ))
                })?;

            // Write DaysAfterInitiation
            writer
                .write_event(Event::Start(BytesStart::new("DaysAfterInitiation")))
                .map_err(|e| {
                    StoreError::Other(format!("Failed to write DaysAfterInitiation start: {}", e))
                })?;
            writer
                .write_event(Event::Text(BytesText::new(&days.to_string())))
                .map_err(|e| {
                    StoreError::Other(format!("Failed to write DaysAfterInitiation text: {}", e))
                })?;
            writer
                .write_event(Event::End(BytesEnd::new("DaysAfterInitiation")))
                .map_err(|e| {
                    StoreError::Other(format!("Failed to write DaysAfterInitiation end: {}", e))
                })?;

            // End AbortIncompleteMultipartUpload
            writer
                .write_event(Event::End(BytesEnd::new("AbortIncompleteMultipartUpload")))
                .map_err(|e| {
                    StoreError::Other(format!(
                        "Failed to write AbortIncompleteMultipartUpload end: {}",
                        e
                    ))
                })?;
        }

        // End Rule
        writer
            .write_event(Event::End(BytesEnd::new("Rule")))
            .map_err(|e| StoreError::Other(format!("Failed to write Rule end: {}", e)))?;
    }

    // End LifecycleConfiguration
    writer
        .write_event(Event::End(BytesEnd::new("LifecycleConfiguration")))
        .map_err(|e| {
            StoreError::Other(format!("Failed to write LifecycleConfiguration end: {}", e))
        })?;

    // Get the XML string
    let result = writer.into_inner().into_inner();
    let xml_string = String::from_utf8(result)
        .map_err(|e| StoreError::Other(format!("Failed to convert XML to UTF-8: {}", e)))?;

    Ok(xml_string)
}
