use super::*;
// ---------------------------------------------------------------------------
// Toggles: enable/disable skills and tools
// ---------------------------------------------------------------------------

/// GET /v1/toggles — get all toggle states
pub(super) async fn handle_toggles_get(State(state): State<GatewayState>) -> impl IntoResponse {
    let path = state.paths.toggles_file();
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Json(serde_json::json!({ "skills": {}, "tools": {} }));
    }
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(val) => Json(val),
            Err(_) => Json(serde_json::json!({ "skills": {}, "tools": {} })),
        },
        Err(_) => Json(serde_json::json!({ "skills": {}, "tools": {} })),
    }
}

#[derive(Deserialize)]
pub(super) struct ToggleUpdateRequest {
    category: String, // "skills" or "tools"
    name: String,
    enabled: bool,
}

/// PUT /v1/toggles — update a single toggle
pub(super) async fn handle_toggles_update(
    State(state): State<GatewayState>,
    Json(req): Json<ToggleUpdateRequest>,
) -> impl IntoResponse {
    if req.category != "skills" && req.category != "tools" {
        return Json(serde_json::json!({ "error": "category must be 'skills' or 'tools'" }));
    }

    let path = state.paths.toggles_file();
    let mut store: serde_json::Value = if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        tokio::fs::read_to_string(&path)
            .await
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or(serde_json::json!({ "skills": {}, "tools": {} }))
    } else {
        serde_json::json!({ "skills": {}, "tools": {} })
    };

    // Ensure category object exists
    if store.get(&req.category).is_none() {
        store[&req.category] = serde_json::json!({});
    }

    // Set the toggle value. If enabled=true, remove the entry (default is enabled).
    // If enabled=false, store false explicitly.
    if req.enabled {
        if let Some(obj) = store[&req.category].as_object_mut() {
            obj.remove(&req.name);
        }
    } else {
        store[&req.category][&req.name] = serde_json::json!(false);
    }

    match tokio::fs::write(
        &path,
        serde_json::to_string_pretty(&store).unwrap_or_default(),
    ).await {
        Ok(_) => Json(serde_json::json!({
            "status": "ok",
            "category": req.category,
            "name": req.name,
            "enabled": req.enabled,
        })),
        Err(e) => Json(serde_json::json!({ "error": format!("{}", e) })),
    }
}
