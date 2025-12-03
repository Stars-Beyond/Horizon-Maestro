//! Horizon-specific instance management routes.
//!
//! This module provides specialized endpoints for managing Horizon game server
//! instances, including automatic environment configuration for Atlas integration.

use rocket::{get, post, put};
use rocket::serde::json::Json;
use rocket::serde::json::serde_json;
use rocket::State;
use std::collections::HashMap;
use bollard::container::{CreateContainerOptions, Config, StartContainerOptions, StopContainerOptions};
use bollard::models::{HostConfig, PortBinding};
use bollard::image::CreateImageOptions;
use futures::stream::TryStreamExt;
use serde::{Deserialize, Serialize};

use crate::routes::app_manager::AppManager;
use crate::routes::models::AppInstance;

/// Default Horizon Docker image (from GitHub Container Registry)
const DEFAULT_HORIZON_IMAGE: &str = "ghcr.io/far-beyond-dev/horizon:main";

/// Default container port for Horizon servers
const DEFAULT_CONTAINER_PORT: u16 = 8080;

/// Request to create a new Horizon server instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonInstanceRequest {
    /// Instance name (optional, generated from region if not provided)
    pub name: Option<String>,
    /// Docker image (defaults to ghcr.io/far-beyond-dev/horizon:main)
    pub image: Option<String>,
    /// Host path to config.toml file (for volume mount)
    pub config_path: Option<String>,
    /// Host port for the server (required)
    pub host_port: u16,
    /// Region coordinates
    pub region: RegionCoordinate,
    /// Region center in world coordinates
    pub center: WorldCoordinate,
    /// Region bounds (half-extent)
    pub bounds_size: f64,
    /// Atlas registration URL
    pub atlas_url: String,
    /// Maximum player capacity
    #[serde(default = "default_capacity")]
    pub capacity: u32,
    /// Additional environment variables
    #[serde(default)]
    pub extra_env: HashMap<String, String>,
}

fn default_capacity() -> u32 {
    1000
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegionCoordinate {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorldCoordinate {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// Response from creating a Horizon instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonInstanceResponse {
    pub success: bool,
    pub instance_id: Option<String>,
    pub name: String,
    pub address: String,
    pub region: RegionCoordinate,
    pub error: Option<String>,
}

/// List all Horizon instances.
#[get("/horizon/instances")]
pub async fn list_horizon_instances(app_manager: &State<AppManager>) -> Json<Vec<AppInstance>> {
    use bollard::container::ListContainersOptions;
    
    let mut instances = Vec::new();
    
    // Filter for containers with Horizon labels
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec!["horizon.type=game-server".to_string()]);
    
    let options = Some(ListContainersOptions {
        all: true,
        filters,
        ..Default::default()
    });
    
    match app_manager.docker.list_containers(options).await {
        Ok(containers) => {
            for container in containers {
                if let (Some(id), Some(image), Some(names), Some(created), Some(status)) = 
                   (container.id, container.image, container.names, container.created, container.status) {
                    if let Some(name) = names.first() {
                        let name = name.trim_start_matches('/').to_string();
                        
                        // Extract environment from labels
                        let mut environment = HashMap::new();
                        if let Some(labels) = container.labels {
                            if let Some(region_x) = labels.get("horizon.region.x") {
                                environment.insert("HORIZON_REGION_X".to_string(), region_x.clone());
                            }
                            if let Some(region_y) = labels.get("horizon.region.y") {
                                environment.insert("HORIZON_REGION_Y".to_string(), region_y.clone());
                            }
                            if let Some(region_z) = labels.get("horizon.region.z") {
                                environment.insert("HORIZON_REGION_Z".to_string(), region_z.clone());
                            }
                        }
                        
                        let app_instance = AppInstance {
                            id: id.clone(),
                            name,
                            image,
                            status,
                            created_at: created.to_string(),
                            ports: Vec::new(),
                            environment,
                            volumes: Vec::new(),
                            agent_id: "current".to_string(),
                        };
                        instances.push(app_instance);
                    }
                }
            }
        },
        Err(e) => {
            eprintln!("Failed to list Horizon containers: {}", e);
        }
    }
    
    Json(instances)
}

/// Create a new Horizon server instance.
#[post("/horizon/instances", format = "json", data = "<request>")]
pub async fn create_horizon_instance(
    request: Json<HorizonInstanceRequest>,
    app_manager: &State<AppManager>,
) -> Json<HorizonInstanceResponse> {
    let req = request.into_inner();
    
    // Generate instance name from region if not provided
    let name = req.name.unwrap_or_else(|| {
        format!("horizon-{}-{}-{}", req.region.x, req.region.y, req.region.z)
    });
    
    let image = req.image.unwrap_or_else(|| DEFAULT_HORIZON_IMAGE.to_string());
    
    // Build environment variables
    let mut env = vec![
        format!("HORIZON_REGION_X={}", req.region.x),
        format!("HORIZON_REGION_Y={}", req.region.y),
        format!("HORIZON_REGION_Z={}", req.region.z),
        format!("HORIZON_CENTER_X={}", req.center.x),
        format!("HORIZON_CENTER_Y={}", req.center.y),
        format!("HORIZON_CENTER_Z={}", req.center.z),
        format!("HORIZON_BOUNDS_SIZE={}", req.bounds_size),
        format!("HORIZON_ATLAS_URL={}", req.atlas_url),
        format!("HORIZON_ATLAS_ENABLED=true"),
        format!("HORIZON_BIND_ADDRESS=0.0.0.0:{}", DEFAULT_CONTAINER_PORT),
        format!("HORIZON_MAX_CONNECTIONS={}", req.capacity),
    ];
    
    // Add extra environment variables
    for (key, value) in &req.extra_env {
        env.push(format!("{}={}", key, value));
    }
    
    // Build labels for filtering
    let mut labels = HashMap::new();
    labels.insert("horizon.type".to_string(), "game-server".to_string());
    labels.insert("horizon.region.x".to_string(), req.region.x.to_string());
    labels.insert("horizon.region.y".to_string(), req.region.y.to_string());
    labels.insert("horizon.region.z".to_string(), req.region.z.to_string());
    labels.insert("horizon.atlas.url".to_string(), req.atlas_url.clone());
    
    // Port bindings
    let mut port_bindings = HashMap::new();
    port_bindings.insert(
        format!("{}/tcp", DEFAULT_CONTAINER_PORT),
        Some(vec![PortBinding {
            host_ip: Some("0.0.0.0".to_string()),
            host_port: Some(req.host_port.to_string()),
        }]),
    );
    
    // Build volume binds for config file
    let binds = req.config_path.as_ref().map(|path| {
        vec![format!("{}:/app/config.toml:ro", path)]
    });

    let host_config = HostConfig {
        port_bindings: Some(port_bindings),
        binds,
        ..Default::default()
    };
    
    // Exposed ports
    let mut exposed_ports = HashMap::new();
    exposed_ports.insert(format!("{}/tcp", DEFAULT_CONTAINER_PORT), HashMap::new());
    
    // Try to pull image first
    let create_image_options = Some(CreateImageOptions {
        from_image: image.clone(),
        ..Default::default()
    });
    
    match app_manager.docker.create_image(create_image_options, None, None).try_collect::<Vec<_>>().await {
        Ok(_) => println!("Successfully pulled image: {}", image),
        Err(e) => println!("Using local image {} (pull failed: {})", image, e),
    }
    
    // Create container config
    let config = Config {
        image: Some(image.clone()),
        env: Some(env),
        labels: Some(labels),
        exposed_ports: Some(exposed_ports),
        host_config: Some(host_config),
        ..Default::default()
    };
    
    let options = CreateContainerOptions {
        name: name.clone(),
        platform: None,
    };
    
    // Create container
    let container_id = match app_manager.docker.create_container(Some(options), config).await {
        Ok(response) => response.id,
        Err(e) => {
            return Json(HorizonInstanceResponse {
                success: false,
                instance_id: None,
                name,
                address: String::new(),
                region: req.region,
                error: Some(format!("Failed to create container: {}", e)),
            });
        }
    };
    
    // Start container
    if let Err(e) = app_manager.docker.start_container(&container_id, None::<StartContainerOptions<String>>).await {
        return Json(HorizonInstanceResponse {
            success: false,
            instance_id: Some(container_id),
            name,
            address: String::new(),
            region: req.region,
            error: Some(format!("Failed to start container: {}", e)),
        });
    }
    
    // Get host address (assume localhost for now)
    let address = format!("127.0.0.1:{}", req.host_port);
    
    println!("[Horizon] Created instance {} at {} for region ({}, {}, {})",
        name, address, req.region.x, req.region.y, req.region.z);
    
    Json(HorizonInstanceResponse {
        success: true,
        instance_id: Some(container_id),
        name,
        address,
        region: req.region,
        error: None,
    })
}

/// Stop a Horizon instance.
#[put("/horizon/instances/<id>/stop")]
pub async fn stop_horizon_instance(
    id: String,
    app_manager: &State<AppManager>,
) -> Json<serde_json::Value> {
    let options = StopContainerOptions { t: 30 };
    
    match app_manager.docker.stop_container(&id, Some(options)).await {
        Ok(_) => {
            println!("[Horizon] Stopped instance {}", id);
            Json(serde_json::json!({
                "success": true,
                "message": "Instance stopped"
            }))
        },
        Err(e) => {
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to stop instance: {}", e)
            }))
        }
    }
}

/// Start a stopped Horizon instance.
#[put("/horizon/instances/<id>/start")]
pub async fn start_horizon_instance(
    id: String,
    app_manager: &State<AppManager>,
) -> Json<serde_json::Value> {
    match app_manager.docker.start_container(&id, None::<StartContainerOptions<String>>).await {
        Ok(_) => {
            println!("[Horizon] Started instance {}", id);
            Json(serde_json::json!({
                "success": true,
                "message": "Instance started"
            }))
        },
        Err(e) => {
            Json(serde_json::json!({
                "success": false,
                "error": format!("Failed to start instance: {}", e)
            }))
        }
    }
}

/// Get Horizon instance details.
#[get("/horizon/instances/<id>")]
pub async fn get_horizon_instance(
    id: String,
    app_manager: &State<AppManager>,
) -> Option<Json<HorizonInstanceDetails>> {
    match app_manager.docker.inspect_container(&id, None).await {
        Ok(container) => {
            let config = container.config?;
            let state = container.state?;
            let name = container.name?.trim_start_matches('/').to_string();
            
            // Parse region from labels
            let labels = config.labels.unwrap_or_default();
            let region = RegionCoordinate {
                x: labels.get("horizon.region.x").and_then(|s| s.parse().ok()).unwrap_or(0),
                y: labels.get("horizon.region.y").and_then(|s| s.parse().ok()).unwrap_or(0),
                z: labels.get("horizon.region.z").and_then(|s| s.parse().ok()).unwrap_or(0),
            };
            
            // Parse environment
            let env: HashMap<String, String> = config.env.unwrap_or_default()
                .iter()
                .filter_map(|e| {
                    let parts: Vec<&str> = e.splitn(2, '=').collect();
                    if parts.len() == 2 {
                        Some((parts[0].to_string(), parts[1].to_string()))
                    } else {
                        None
                    }
                })
                .collect();
            
            let atlas_url = labels.get("horizon.atlas.url").cloned().unwrap_or_default();
            
            Some(Json(HorizonInstanceDetails {
                id: container.id.unwrap_or(id),
                name,
                image: config.image.unwrap_or_default(),
                status: state.status.map(|s| s.to_string()).unwrap_or_else(|| "unknown".to_string()),
                running: state.running.unwrap_or(false),
                region,
                atlas_url,
                environment: env,
                created_at: container.created.unwrap_or_default(),
            }))
        },
        Err(_) => None
    }
}

/// Detailed Horizon instance information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonInstanceDetails {
    pub id: String,
    pub name: String,
    pub image: String,
    pub status: String,
    pub running: bool,
    pub region: RegionCoordinate,
    pub atlas_url: String,
    pub environment: HashMap<String, String>,
    pub created_at: String,
}
