use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use std::collections::HashMap;

use crate::{
    adapters::inbound::http::{
        dto::{
            ApplicableActionDto, ErrorResponseDto, EvaluateLifecycleDto, LifecycleConfigurationDto,
            LifecycleEvaluationResponseDto, LifecycleRuleDto, SuccessResponseDto,
        },
        router::AppState,
    },
    domain::{
        models::EvaluateLifecycleRequest,
        value_objects::{BucketName, ObjectKey},
    },
};

/// Handle setting lifecycle configuration for a bucket
pub async fn set_lifecycle_configuration(
    State(app_state): State<AppState>,
    Path(bucket_name): Path<String>,
    Json(config_dto): Json<LifecycleConfigurationDto>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Convert DTO to domain model
    let config = config_dto
        .try_into()
        .map_err(|e: crate::domain::errors::ValidationError| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponseDto::bad_request(&format!(
                    "Invalid configuration: {}",
                    e
                ))),
            )
        })?;

    // Set the configuration
    lifecycle_service
        .set_lifecycle_configuration(&bucket, config)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new(
            "Lifecycle configuration set successfully",
        )),
    ))
}

/// Handle getting lifecycle configuration for a bucket
pub async fn get_lifecycle_configuration(
    State(app_state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<Json<LifecycleConfigurationDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Get the configuration
    let config = lifecycle_service
        .get_lifecycle_configuration(&bucket)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    match config {
        Some(config) => Ok(Json(config.into())),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponseDto::bad_request(
                "Lifecycle configuration not found",
            )),
        )),
    }
}

/// Handle deleting lifecycle configuration for a bucket
pub async fn delete_lifecycle_configuration(
    State(app_state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Delete the configuration
    lifecycle_service
        .delete_lifecycle_configuration(&bucket)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new(
            "Lifecycle configuration deleted successfully",
        )),
    ))
}

/// Handle evaluating lifecycle rules for an object
pub async fn evaluate_object_lifecycle(
    State(app_state): State<AppState>,
    Json(request_dto): Json<EvaluateLifecycleDto>,
) -> Result<Json<LifecycleEvaluationResponseDto>, (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Create object key
    let object_key = ObjectKey::new(request_dto.key).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid object key: {}",
                e
            ))),
        )
    })?;

    // Create evaluation request
    let request = EvaluateLifecycleRequest {
        key: object_key,
        object_created_at: request_dto
            .object_created_at
            .map(|dt| dt.into())
            .unwrap_or_else(std::time::SystemTime::now),
        object_tags: request_dto.object_tags.unwrap_or_default(),
        is_delete_marker: request_dto.is_delete_marker.unwrap_or(false),
        is_current_version: request_dto.is_current_version.unwrap_or(true),
    };

    // Evaluate lifecycle
    let result = lifecycle_service
        .evaluate_object_lifecycle(request)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    // Convert to DTO
    let actions_dto: Vec<ApplicableActionDto> = result
        .actions_to_apply
        .into_iter()
        .map(|action| {
            let mut details = HashMap::new();

            // Add action-specific details
            match &action.action {
                crate::domain::models::LifecycleAction::Expiration { days, date } => {
                    if let Some(days) = days {
                        details.insert(
                            "days".to_string(),
                            serde_json::Value::Number((*days).into()),
                        );
                    }
                    if let Some(date) = date {
                        details.insert(
                            "date".to_string(),
                            serde_json::Value::String(date.to_rfc3339()),
                        );
                    }
                }
                crate::domain::models::LifecycleAction::Transition {
                    days,
                    date,
                    storage_class,
                } => {
                    if let Some(days) = days {
                        details.insert(
                            "days".to_string(),
                            serde_json::Value::Number((*days).into()),
                        );
                    }
                    if let Some(date) = date {
                        details.insert(
                            "date".to_string(),
                            serde_json::Value::String(date.to_rfc3339()),
                        );
                    }
                    details.insert(
                        "storage_class".to_string(),
                        serde_json::Value::String(storage_class.as_str().to_string()),
                    );
                }
                _ => {}
            }

            ApplicableActionDto {
                rule_id: action.rule_id,
                action_type: format!("{:?}", action.action),
                reason: action.reason,
                details,
            }
        })
        .collect();

    Ok(Json(LifecycleEvaluationResponseDto {
        actions_to_apply: actions_dto,
    }))
}

/// Handle adding a new lifecycle rule
pub async fn add_lifecycle_rule(
    State(app_state): State<AppState>,
    Path(bucket_name): Path<String>,
    Json(rule_dto): Json<LifecycleRuleDto>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Convert DTO to domain model
    let rule = rule_dto
        .try_into()
        .map_err(|e: crate::domain::errors::ValidationError| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponseDto::bad_request(&format!(
                    "Invalid rule: {}",
                    e
                ))),
            )
        })?;

    // Add the rule
    lifecycle_service
        .add_rule(&bucket, rule)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    Ok((
        StatusCode::CREATED,
        Json(SuccessResponseDto::new("Lifecycle rule added successfully")),
    ))
}

/// Handle removing a lifecycle rule
pub async fn remove_lifecycle_rule(
    State(app_state): State<AppState>,
    Path((bucket_name, rule_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Remove the rule
    lifecycle_service
        .remove_rule(&bucket, &rule_id)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new(
            "Lifecycle rule removed successfully",
        )),
    ))
}

/// Handle enabling a lifecycle rule
pub async fn enable_lifecycle_rule(
    State(app_state): State<AppState>,
    Path((bucket_name, rule_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Enable the rule
    lifecycle_service
        .enable_rule(&bucket, &rule_id)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new(
            "Lifecycle rule enabled successfully",
        )),
    ))
}

/// Handle disabling a lifecycle rule
pub async fn disable_lifecycle_rule(
    State(app_state): State<AppState>,
    Path((bucket_name, rule_id)): Path<(String, String)>,
) -> Result<(StatusCode, Json<SuccessResponseDto>), (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Disable the rule
    lifecycle_service
        .disable_rule(&bucket, &rule_id)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    Ok((
        StatusCode::OK,
        Json(SuccessResponseDto::new(
            "Lifecycle rule disabled successfully",
        )),
    ))
}

/// Handle processing bucket lifecycle
pub async fn process_bucket_lifecycle(
    State(app_state): State<AppState>,
    Path(bucket_name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponseDto>)> {
    let lifecycle_service = &app_state.lifecycle_service;

    // Validate bucket name
    let bucket = BucketName::new(bucket_name).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponseDto::bad_request(&format!(
                "Invalid bucket name: {}",
                e
            ))),
        )
    })?;

    // Process lifecycle
    let results = lifecycle_service
        .process_bucket_lifecycle(&bucket)
        .await
        .map_err(|e| {
            let status_code = StatusCode::from(e.clone());
            (status_code, Json(ErrorResponseDto::from_lifecycle_error(e)))
        })?;

    // Convert results to JSON
    let response = serde_json::json!({
        "bucket": results.bucket.as_str(),
        "objects_processed": results.objects_processed,
        "objects_affected": results.objects_affected,
        "actions_applied": results.actions_applied,
        "errors": results.errors.iter().map(|e| serde_json::json!({
            "object_key": e.object_key.as_str(),
            "rule_id": e.rule_id,
            "error": e.error
        })).collect::<Vec<_>>(),
        "duration_ms": results.duration.as_millis()
    });

    Ok(Json(response))
}
