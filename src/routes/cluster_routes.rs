//! Cluster management routes.
//!
//! These routes expose the cluster orchestrator functionality via REST API.

use rocket::{get, post, State};
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::orchestrator::{ClusterOrchestrator, ClusterStatus};
use crate::routes::horizon_routes::RegionCoordinate;

/// Request to bootstrap the cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapRequest {
    /// Override initial regions (optional)
    pub regions: Option<Vec<RegionCoordinate>>,
}

/// Response from bootstrap operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapResponse {
    pub success: bool,
    pub message: String,
    pub status: Option<ClusterStatus>,
}

/// Request to scale a new region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleRequest {
    pub region: RegionCoordinate,
}

/// Response from scale operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleResponse {
    pub success: bool,
    pub container_id: Option<String>,
    pub message: String,
}

/// Get cluster status.
#[get("/cluster/status")]
pub async fn cluster_status(
    orchestrator: &State<Arc<ClusterOrchestrator>>,
) -> Json<ClusterStatus> {
    Json(orchestrator.status().await)
}

/// Bootstrap the cluster (deploy Atlas + initial Horizon instances).
#[post("/cluster/bootstrap")]
pub async fn cluster_bootstrap(
    orchestrator: &State<Arc<ClusterOrchestrator>>,
) -> Json<BootstrapResponse> {
    match orchestrator.bootstrap().await {
        Ok(()) => {
            let status = orchestrator.status().await;
            Json(BootstrapResponse {
                success: true,
                message: "Cluster bootstrapped successfully".to_string(),
                status: Some(status),
            })
        }
        Err(e) => Json(BootstrapResponse {
            success: false,
            message: format!("Bootstrap failed: {}", e),
            status: None,
        }),
    }
}

/// Scale: deploy a new region on demand.
#[post("/cluster/scale", format = "json", data = "<request>")]
pub async fn cluster_scale(
    request: Json<ScaleRequest>,
    orchestrator: &State<Arc<ClusterOrchestrator>>,
) -> Json<ScaleResponse> {
    match orchestrator.scale_region(request.region).await {
        Ok(container_id) => Json(ScaleResponse {
            success: true,
            container_id: Some(container_id),
            message: format!("Region {:?} deployed", request.region),
        }),
        Err(e) => Json(ScaleResponse {
            success: false,
            container_id: None,
            message: e,
        }),
    }
}

/// List all deployed instances.
#[get("/cluster/instances")]
pub async fn cluster_instances(
    orchestrator: &State<Arc<ClusterOrchestrator>>,
) -> Json<Vec<ClusterInstance>> {
    let instances = orchestrator.list_instances().await;
    Json(instances.into_iter().map(|i| ClusterInstance {
        container_id: i.container_id,
        name: i.name,
        instance_type: format!("{:?}", i.instance_type),
        address: i.address,
        region: i.region,
        healthy: i.healthy,
    }).collect())
}

/// Cluster instance info for API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterInstance {
    pub container_id: String,
    pub name: String,
    pub instance_type: String,
    pub address: String,
    pub region: Option<RegionCoordinate>,
    pub healthy: bool,
}
