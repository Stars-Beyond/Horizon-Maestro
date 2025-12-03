//! Cluster Orchestrator for automatic deployment and management.
//!
//! This module handles the automatic bootstrap and scaling of the Horizon cluster:
//! - Deploys Atlas instances (each manages a region of adjacent Horizon servers)
//! - Deploys initial Horizon instance(s) at startup
//! - Monitors health and restarts failed instances
//! - Scales on demand when Atlas requests new regions

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::routes::instances::AppManager;
use crate::routes::horizon_routes::{HorizonInstanceRequest, RegionCoordinate, WorldCoordinate};

/// Configuration for the cluster orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterConfig {
    /// Whether to auto-bootstrap on startup
    pub auto_bootstrap: bool,
    /// Atlas Docker image
    pub atlas_image: String,
    /// Horizon Docker image
    pub horizon_image: String,
    /// Path to Horizon config.toml on the host (for volume mount)
    pub horizon_config_path: Option<String>,
    /// Base port for Horizon instances (increments per instance)
    pub horizon_base_port: u16,
    /// Atlas proxy port
    pub atlas_proxy_port: u16,
    /// Atlas API port
    pub atlas_api_port: u16,
    /// Region size (world units per region)
    pub region_size: f64,
    /// Initial regions to spawn (e.g., just origin)
    pub initial_regions: Vec<RegionCoordinate>,
    /// Health check interval in seconds
    pub health_check_interval: u64,
    /// Maximum Horizon instances
    pub max_horizon_instances: u32,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            auto_bootstrap: true,
            atlas_image: "ghcr.io/far-beyond-dev/horizon-atlas:main".to_string(),
            horizon_image: "ghcr.io/far-beyond-dev/horizon:main".to_string(),
            horizon_config_path: None,
            horizon_base_port: 8080,
            atlas_proxy_port: 9000,
            atlas_api_port: 9001,
            region_size: 1000.0,
            initial_regions: vec![RegionCoordinate { x: 0, y: 0, z: 0 }],
            health_check_interval: 30,
            max_horizon_instances: 100,
        }
    }
}

impl ClusterConfig {
    /// Load config from environment variables.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        
        if let Ok(v) = std::env::var("MAESTRO_AUTO_BOOTSTRAP") {
            config.auto_bootstrap = v == "true" || v == "1";
        }
        if let Ok(v) = std::env::var("MAESTRO_ATLAS_IMAGE") {
            config.atlas_image = v;
        }
        if let Ok(v) = std::env::var("MAESTRO_HORIZON_IMAGE") {
            config.horizon_image = v;
        }
        if let Ok(v) = std::env::var("MAESTRO_HORIZON_CONFIG_PATH") {
            config.horizon_config_path = Some(v);
        }
        if let Ok(v) = std::env::var("MAESTRO_HORIZON_BASE_PORT") {
            if let Ok(port) = v.parse() {
                config.horizon_base_port = port;
            }
        }
        if let Ok(v) = std::env::var("MAESTRO_REGION_SIZE") {
            if let Ok(size) = v.parse() {
                config.region_size = size;
            }
        }
        
        config
    }
}

/// Tracks a deployed instance.
#[derive(Debug, Clone)]
pub struct DeployedInstance {
    pub container_id: String,
    pub name: String,
    pub instance_type: InstanceType,
    pub address: String,
    pub region: Option<RegionCoordinate>,
    pub healthy: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceType {
    Atlas,
    Horizon,
}

/// The cluster orchestrator manages automatic deployment and scaling.
pub struct ClusterOrchestrator {
    config: ClusterConfig,
    app_manager: Arc<AppManager>,
    /// Deployed instances (container_id -> instance)
    instances: Arc<RwLock<HashMap<String, DeployedInstance>>>,
    /// Next available port for Horizon
    next_horizon_port: Arc<RwLock<u16>>,
    /// Atlas instance ID (if deployed)
    atlas_id: Arc<RwLock<Option<String>>>,
}

impl ClusterOrchestrator {
    /// Create a new orchestrator.
    pub fn new(app_manager: Arc<AppManager>, config: ClusterConfig) -> Self {
        let base_port = config.horizon_base_port;
        Self {
            config,
            app_manager,
            instances: Arc::new(RwLock::new(HashMap::new())),
            next_horizon_port: Arc::new(RwLock::new(base_port)),
            atlas_id: Arc::new(RwLock::new(None)),
        }
    }

    /// Bootstrap the cluster: deploy Atlas + initial Horizon instances.
    pub async fn bootstrap(&self) -> Result<(), String> {
        println!("[Orchestrator] Starting cluster bootstrap...");
        
        // Step 1: Deploy Atlas
        let atlas_id = self.deploy_atlas().await?;
        println!("[Orchestrator] ✅ Atlas deployed: {}", atlas_id);
        
        // Wait for Atlas to be ready
        tokio::time::sleep(Duration::from_secs(3)).await;
        
        // Step 2: Deploy initial Horizon instances
        for region in &self.config.initial_regions.clone() {
            match self.deploy_horizon(*region).await {
                Ok(id) => println!("[Orchestrator] ✅ Horizon {:?} deployed: {}", region, id),
                Err(e) => println!("[Orchestrator] ❌ Failed to deploy Horizon {:?}: {}", region, e),
            }
        }
        
        println!("[Orchestrator] Bootstrap complete!");
        Ok(())
    }

    /// Deploy an Atlas instance.
    pub async fn deploy_atlas(&self) -> Result<String, String> {
        use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
        use bollard::models::{HostConfig, PortBinding};
        use bollard::image::CreateImageOptions;
        use futures::stream::TryStreamExt;
        
        let name = "atlas-primary".to_string();
        let image = self.config.atlas_image.clone();
        
        // Build environment
        let env = vec![
            format!("ATLAS_PROXY_ADDR=0.0.0.0:{}", self.config.atlas_proxy_port),
            format!("ATLAS_API_ADDR=0.0.0.0:{}", self.config.atlas_api_port),
            format!("ATLAS_MAESTRO_URL=http://host.docker.internal:8000"),
            format!("ATLAS_REGION_SIZE={}", self.config.region_size),
            "ATLAS_AUTO_SCALE=true".to_string(),
        ];
        
        // Port bindings
        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            format!("{}/tcp", self.config.atlas_proxy_port),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(self.config.atlas_proxy_port.to_string()),
            }]),
        );
        port_bindings.insert(
            format!("{}/tcp", self.config.atlas_api_port),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(self.config.atlas_api_port.to_string()),
            }]),
        );
        
        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            ..Default::default()
        };
        
        // Labels (include compose-compatible labels for Docker Desktop grouping)
        let mut labels = HashMap::new();
        labels.insert("horizon.type".to_string(), "atlas".to_string());
        labels.insert("horizon.cluster".to_string(), "primary".to_string());
        labels.insert("com.docker.compose.project".to_string(), "horizon-cluster".to_string());
        labels.insert("com.docker.compose.service".to_string(), "atlas".to_string());
        
        // Try to pull image
        let create_image_options = Some(CreateImageOptions {
            from_image: image.clone(),
            ..Default::default()
        });
        
        match self.app_manager.docker.create_image(create_image_options, None, None).try_collect::<Vec<_>>().await {
            Ok(_) => println!("[Orchestrator] Pulled image: {}", image),
            Err(e) => println!("[Orchestrator] Using local image {} (pull: {})", image, e),
        }
        
        // Exposed ports
        let mut exposed_ports = HashMap::new();
        exposed_ports.insert(format!("{}/tcp", self.config.atlas_proxy_port), HashMap::new());
        exposed_ports.insert(format!("{}/tcp", self.config.atlas_api_port), HashMap::new());
        
        let config = Config {
            image: Some(image),
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
        let container_id = self.app_manager.docker
            .create_container(Some(options), config)
            .await
            .map_err(|e| format!("Failed to create Atlas container: {}", e))?
            .id;
        
        // Start container
        self.app_manager.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| format!("Failed to start Atlas container: {}", e))?;
        
        // Track instance
        {
            let mut instances = self.instances.write().await;
            instances.insert(container_id.clone(), DeployedInstance {
                container_id: container_id.clone(),
                name: name.clone(),
                instance_type: InstanceType::Atlas,
                address: format!("127.0.0.1:{}", self.config.atlas_api_port),
                region: None,
                healthy: true,
            });
        }
        
        *self.atlas_id.write().await = Some(container_id.clone());
        
        Ok(container_id)
    }

    /// Deploy a Horizon instance for a specific region.
    pub async fn deploy_horizon(&self, region: RegionCoordinate) -> Result<String, String> {
        // Get next available port
        let port = {
            let mut next_port = self.next_horizon_port.write().await;
            let port = *next_port;
            *next_port += 1;
            port
        };
        
        // Calculate region center
        let center = WorldCoordinate {
            x: (region.x as f64 + 0.5) * self.config.region_size,
            y: (region.y as f64 + 0.5) * self.config.region_size,
            z: (region.z as f64 + 0.5) * self.config.region_size,
        };
        
        let request = HorizonInstanceRequest {
            name: None, // Auto-generate from region
            image: Some(self.config.horizon_image.clone()),
            config_path: self.config.horizon_config_path.clone(),
            host_port: port,
            region,
            center,
            bounds_size: self.config.region_size / 2.0,
            atlas_url: format!("http://host.docker.internal:{}", self.config.atlas_api_port),
            capacity: 1000,
            extra_env: HashMap::new(),
        };
        
        // Use the existing horizon instance creation logic
        self.create_horizon_internal(request).await
    }

    /// Internal method to create a Horizon instance.
    async fn create_horizon_internal(&self, req: HorizonInstanceRequest) -> Result<String, String> {
        use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
        use bollard::models::{HostConfig, PortBinding};
        use bollard::image::CreateImageOptions;
        use futures::stream::TryStreamExt;
        
        let name = req.name.unwrap_or_else(|| {
            format!("horizon-{}-{}-{}", req.region.x, req.region.y, req.region.z)
        });
        
        let image = req.image.unwrap_or_else(|| self.config.horizon_image.clone());
        
        // Build environment
        let env = vec![
            format!("HORIZON_REGION_X={}", req.region.x),
            format!("HORIZON_REGION_Y={}", req.region.y),
            format!("HORIZON_REGION_Z={}", req.region.z),
            format!("HORIZON_CENTER_X={}", req.center.x),
            format!("HORIZON_CENTER_Y={}", req.center.y),
            format!("HORIZON_CENTER_Z={}", req.center.z),
            format!("HORIZON_BOUNDS_SIZE={}", req.bounds_size),
            format!("HORIZON_ATLAS_URL={}", req.atlas_url),
            "HORIZON_ATLAS_ENABLED=true".to_string(),
            "HORIZON_BIND_ADDRESS=0.0.0.0:8080".to_string(),
            format!("HORIZON_MAX_CONNECTIONS={}", req.capacity),
        ];
        
        // Port bindings (container port 8080 -> host port)
        let mut port_bindings = HashMap::new();
        port_bindings.insert(
            "8080/tcp".to_string(),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(req.host_port.to_string()),
            }]),
        );
        
        // Build volume binds for config file
        let binds = self.config.horizon_config_path.as_ref().map(|path| {
            vec![format!("{}:/app/config.toml:ro", path)]
        });

        let host_config = HostConfig {
            port_bindings: Some(port_bindings),
            binds,
            ..Default::default()
        };
        
        // Labels (include compose-compatible labels for Docker Desktop grouping)
        let mut labels = HashMap::new();
        labels.insert("horizon.type".to_string(), "game-server".to_string());
        labels.insert("horizon.region.x".to_string(), req.region.x.to_string());
        labels.insert("horizon.region.y".to_string(), req.region.y.to_string());
        labels.insert("horizon.region.z".to_string(), req.region.z.to_string());
        labels.insert("com.docker.compose.project".to_string(), "horizon-cluster".to_string());
        labels.insert("com.docker.compose.service".to_string(), name.clone());
        
        // Try to pull image
        let create_image_options = Some(CreateImageOptions {
            from_image: image.clone(),
            ..Default::default()
        });
        
        match self.app_manager.docker.create_image(create_image_options, None, None).try_collect::<Vec<_>>().await {
            Ok(_) => println!("[Orchestrator] Pulled image: {}", image),
            Err(e) => println!("[Orchestrator] Using local image {} (pull: {})", image, e),
        }
        
        // Exposed ports
        let mut exposed_ports = HashMap::new();
        exposed_ports.insert("8080/tcp".to_string(), HashMap::new());
        
        let config = Config {
            image: Some(image),
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
        let container_id = self.app_manager.docker
            .create_container(Some(options), config)
            .await
            .map_err(|e| format!("Failed to create Horizon container: {}", e))?
            .id;
        
        // Start container
        self.app_manager.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| format!("Failed to start Horizon container: {}", e))?;
        
        // Track instance
        {
            let mut instances = self.instances.write().await;
            instances.insert(container_id.clone(), DeployedInstance {
                container_id: container_id.clone(),
                name: name.clone(),
                instance_type: InstanceType::Horizon,
                address: format!("127.0.0.1:{}", req.host_port),
                region: Some(req.region),
                healthy: true,
            });
        }
        
        Ok(container_id)
    }

    /// Get all deployed instances.
    pub async fn list_instances(&self) -> Vec<DeployedInstance> {
        self.instances.read().await.values().cloned().collect()
    }

    /// Check if a region is already deployed.
    pub async fn is_region_deployed(&self, region: &RegionCoordinate) -> bool {
        let instances = self.instances.read().await;
        instances.values().any(|i| {
            i.instance_type == InstanceType::Horizon && i.region.as_ref() == Some(region)
        })
    }

    /// Scale: deploy a new region on demand.
    pub async fn scale_region(&self, region: RegionCoordinate) -> Result<String, String> {
        if self.is_region_deployed(&region).await {
            return Err(format!("Region {:?} already deployed", region));
        }
        
        // Check max instances
        let current_count = self.instances.read().await
            .values()
            .filter(|i| i.instance_type == InstanceType::Horizon)
            .count() as u32;
        
        if current_count >= self.config.max_horizon_instances {
            return Err("Maximum Horizon instances reached".to_string());
        }
        
        self.deploy_horizon(region).await
    }

    /// Get cluster status.
    pub async fn status(&self) -> ClusterStatus {
        let instances = self.instances.read().await;
        
        let horizon_count = instances.values()
            .filter(|i| i.instance_type == InstanceType::Horizon)
            .count() as u32;
        
        let atlas_deployed = instances.values()
            .any(|i| i.instance_type == InstanceType::Atlas);
        
        ClusterStatus {
            atlas_deployed,
            horizon_count,
            max_horizon: self.config.max_horizon_instances,
            regions: instances.values()
                .filter_map(|i| i.region)
                .collect(),
        }
    }
}

/// Cluster status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterStatus {
    pub atlas_deployed: bool,
    pub horizon_count: u32,
    pub max_horizon: u32,
    pub regions: Vec<RegionCoordinate>,
}
