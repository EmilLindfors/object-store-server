use axum::{
    Json,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use bytes::Bytes;
use serde::Deserialize;
use std::sync::Arc;

use crate::{
    adapters::inbound::http::{
        AppState,
        dto::{ErrorResponseDto, ListVersionsResponseDto, SuccessResponseDto, VersionedObjectDto},
    },
    domain::{
        models::{CreateObjectRequest, DeleteVersionRequest, GetObjectRequest},
        value_objects::{ObjectKey, VersionId},
    },
    ports::services::VersioningService,
};

#[derive(Debug, Deserialize)]
pub struct ListVersionsQuery {
    pub max_keys: Option<usize>,
    pub key_marker: Option<String>,
    pub version_id_marker: Option<String>,
}

/// Handle creating a versioned object
pub async fn put_versioned_object(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponseDto>)> {
    // Extract content type from headers
    let content_type = headers.get("content-type").and_then(|ct| ct.to_str().ok());

    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Create request
    let request = CreateObjectRequest {
        key: object_key.clone(),
        data: body.to_vec(),
        content_type: content_type.map(|s| s.to_string()),
        custom_metadata: Default::default(),
    };

    // Create versioned object
    let versioned_object = app_state
        .versioning_service
        .create_versioned_object(request)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    let response = serde_json::json!({
        "message": "Versioned object created successfully",
        "version_id": versioned_object.version_id.as_str(),
        "key": object_key.as_str()
    });

    Ok(Json(response))
}

/// Handle getting a specific version of an object
pub async fn get_versioned_object(
    State(app_state): State<AppState>,
    Path((key, version_id)): Path<(String, String)>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponseDto>)> {
    // Create object key and version ID
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    let version = VersionId::new(version_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid version ID: {}",
                e
            ))),
        )
    })?;

    // Create request for getting versioned object
    let request = GetObjectRequest {
        key: object_key,
        version_id: Some(version),
    };

    // Get the versioned object
    let versioned_object = app_state
        .versioning_service
        .get_object(request)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    // Return the object data with version headers
    let content_type = versioned_object
        .metadata
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("x-amz-version-id", versioned_object.version_id.as_str())
        .body(Body::from(versioned_object.data))
        .unwrap())
}

/// Handle getting the latest version of an object
pub async fn get_latest_object(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Response<Body>, (StatusCode, Json<ErrorResponseDto>)> {
    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Get the latest version (no specific version requested)
    let request = GetObjectRequest {
        key: object_key,
        version_id: None,
    };

    let versioned_object = app_state
        .versioning_service
        .get_object(request)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    // Return the object data with version headers
    let content_type = versioned_object
        .metadata
        .content_type
        .as_deref()
        .unwrap_or("application/octet-stream");

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", content_type)
        .header("x-amz-version-id", versioned_object.version_id.as_str())
        .body(Body::from(versioned_object.data))
        .unwrap())
}

/// Handle deleting a specific version
pub async fn delete_versioned_object(
    State(app_state): State<AppState>,
    Path((key, version_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    // Create object key and version ID
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    let version = VersionId::new(version_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid version ID: {}",
                e
            ))),
        )
    })?;

    // Create delete request
    let request = DeleteVersionRequest {
        key: object_key,
        version_id: version,
    };

    // Delete the version
    app_state
        .versioning_service
        .delete_version(request)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new("Version deleted successfully")),
    ))
}

/// Handle listing all versions of an object
pub async fn list_object_versions(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
    Query(params): Query<ListVersionsQuery>,
) -> Result<Json<ListVersionsResponseDto>, (StatusCode, Json<ErrorResponseDto>)> {
    // Create object key
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Get all versions
    let versions = app_state
        .versioning_service
        .list_versions(&object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    // Convert to DTOs
    let version_dtos: Vec<VersionedObjectDto> = versions
        .versions
        .into_iter()
        .map(|version_info| VersionedObjectDto {
            key: object_key.as_str().to_string(),
            version_id: version_info.version_id.as_str().to_string(),
            size: version_info.size,
            last_modified: chrono::DateTime::from(version_info.last_modified),
            etag: version_info.etag,
            is_latest: version_info.is_latest,
        })
        .collect();

    // Apply pagination if specified
    let (truncated_versions, is_truncated) = if let Some(max_keys) = params.max_keys {
        let truncated: Vec<_> = version_dtos.into_iter().take(max_keys).collect();
        let is_truncated = truncated.len() >= max_keys;
        (truncated, is_truncated)
    } else {
        (version_dtos, false)
    };

    Ok(Json(ListVersionsResponseDto {
        versions: truncated_versions,
        delete_markers: Vec::new(), // Would need to track delete markers
        is_truncated,
        next_key_marker: None,
        next_version_id_marker: None,
    }))
}

/// Handle copying a specific version to a new object
pub async fn copy_versioned_object(
    State(app_state): State<AppState>,
    Path((source_key, source_version_id, dest_key)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponseDto>)> {
    // Create object keys and version ID
    let source_object_key = ObjectKey::new(source_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid source key: {}",
                e
            ))),
        )
    })?;

    let source_version = VersionId::new(source_version_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid source version ID: {}",
                e
            ))),
        )
    })?;

    let dest_object_key = ObjectKey::new(dest_key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid destination key: {}",
                e
            ))),
        )
    })?;

    // Copy the version
    let new_version_id = app_state
        .versioning_service
        .copy_version(&source_object_key, &source_version, &dest_object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    let response = serde_json::json!({
        "message": "Version copied successfully",
        "source_key": source_object_key.as_str(),
        "source_version_id": source_version.as_str(),
        "destination_key": dest_object_key.as_str(),
        "new_version_id": new_version_id.as_str()
    });

    Ok(Json(response))
}

/// Handle checking if a specific version exists
pub async fn head_versioned_object(
    State(app_state): State<AppState>,
    Path((key, version_id)): Path<(String, String)>,
) -> Result<(StatusCode, HeaderMap), (StatusCode, Json<ErrorResponseDto>)> {
    // Create object key and version ID
    let object_key = ObjectKey::new(key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    let version = VersionId::new(version_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid version ID: {}",
                e
            ))),
        )
    })?;

    // Check if version exists
    let exists = app_state
        .versioning_service
        .version_exists(&object_key, &version)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    if !exists {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponseDto::bad_request("Version not found")),
        ));
    }

    let mut headers = HeaderMap::new();
    headers.insert("x-amz-version-id", version.as_str().parse().unwrap());
    headers.insert("content-type", "application/octet-stream".parse().unwrap());

    Ok((StatusCode::OK, headers))
}

/// Handle restoring a previous version as the latest
pub async fn restore_version(
    State(app_state): State<AppState>,
    Path((key, version_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponseDto>)> {
    // Create object key and version ID
    let object_key = ObjectKey::new(key.clone()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    let version = VersionId::new(version_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid version ID: {}",
                e
            ))),
        )
    })?;

    // Restore the version by copying it as a new version
    let new_version_id = app_state
        .versioning_service
        .copy_version(&object_key, &version, &object_key)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_storage_error(e)))
        })?;

    let response = serde_json::json!({
        "message": "Version restored successfully",
        "key": key,
        "restored_version_id": version.as_str(),
        "new_version_id": new_version_id.as_str()
    });

    Ok(Json(response))
}
