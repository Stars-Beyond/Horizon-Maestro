use colored::Colorize;
use rocket::routes;
use std::sync::Arc;

pub mod routes;
pub mod orchestrator;

use routes::{index, instances, horizon_routes, cluster_routes};
use routes::instances::AppManager;
use orchestrator::{ClusterOrchestrator, ClusterConfig};

mod agent;
use agent::Agent;



const BANNER: &str = r#"
  _   _            _                  __  __                 _             
 | | | | ___  _ __(_)_______  _ __   |  \/  | __ _  ___  ___| |_ _ __ ___  
 | |_| |/ _ \| '__| |_  / _ \| '_ \  | |\/| |/ _` |/ _ \/ __| __| '__/ _ \ 
 |  _  | (_) | |  | |/ / (_) | | | | | |  | | (_| |  __/\__ \ |_| | | (_) |
 |_| |_|\___/|_|  |_/___\___/|_| |_| |_|  |_|\__,_|\___||___/\__|_|  \___/ 
                              Version: {}
"#;
#[rocket::main]
async fn main() -> Result<(), rocket::Error> {
    println!("{}", BANNER.replace("{}", &env!("CARGO_PKG_VERSION")));
    let agent = Agent::new("Horizon-Maestro 1".to_string(), env!("CARGO_PKG_VERSION").to_string());
    println!("+-----------------------------------------------------------------");
    println!("| Selected UUID for agent: {}", agent.id().to_string().bright_green());
    println!("| Agent name: {}", agent.name().bright_blue());
    println!("+-----------------------------------------------------------------");

    let routes = routes![
        index::     index,
        instances:: list_instances,
        instances:: get_instance,
        instances:: create_instance,
        instances:: start_instance,
        instances:: stop_instance,
        instances:: restart_instance,
        instances:: update_instance,
        instances:: delete_instance,
        instances:: list_images,
        instances:: stream_events,
        instances:: health_check,
        instances:: get_instance_logs,
        instances:: get_instance_stats,
        instances:: pause_instance,
        instances:: unpause_instance,
        instances:: inspect_instance,
        instances:: list_volumes,
        instances:: create_volume,
        instances:: delete_volume,
        instances:: list_networks,
        instances:: create_network,
        instances:: delete_network,
        instances:: connect_instance_to_network,
        instances:: disconnect_instance_from_network,
        instances:: get_agent_info,
        // Horizon-specific routes
        horizon_routes:: list_horizon_instances,
        horizon_routes:: create_horizon_instance,
        horizon_routes:: get_horizon_instance,
        horizon_routes:: start_horizon_instance,
        horizon_routes:: stop_horizon_instance,
        // Cluster orchestration routes
        cluster_routes:: cluster_status,
        cluster_routes:: cluster_bootstrap,
        cluster_routes:: cluster_scale,
        cluster_routes:: cluster_instances
    ];

    let routes_clone = routes.clone();
    let app_manager = match AppManager::new() {
        Ok(manager) => manager,
        Err(e) => {
            eprintln!("Failed to initialize AppManager: {}", e);
            std::process::exit(1);
        }
    };
    
    // Create a shared reference for the orchestrator
    let app_manager_arc = Arc::new(AppManager::new().expect("Already validated"));

    // Create cluster orchestrator
    let cluster_config = ClusterConfig::from_env();
    let auto_bootstrap = cluster_config.auto_bootstrap;
    let orchestrator = Arc::new(ClusterOrchestrator::new(app_manager_arc, cluster_config));

    let rocket_instance = rocket::build()
        .mount("/", routes)
        .configure(rocket::Config {
            address: "0.0.0.0".parse().unwrap(),
            ..rocket::Config::default()
        })
        .manage(routes_clone)
        .manage(app_manager)  // Routes use this directly
        .manage(orchestrator.clone());

    // Collect routes information before launch
    index::collect_routes(&rocket_instance);
    
    // Auto-bootstrap cluster if enabled
    if auto_bootstrap {
        println!();
        println!("{}", "🚀 Auto-bootstrap enabled - deploying cluster...".bright_cyan());
        let orch = orchestrator.clone();
        tokio::spawn(async move {
            // Wait for Rocket to start
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            if let Err(e) = orch.bootstrap().await {
                eprintln!("{} {}", "❌ Auto-bootstrap failed:".bright_red(), e);
            } else {
                println!("{}", "✅ Cluster bootstrap complete!".bright_green());
            }
        });
    }
    
    // Launch the server
    let _server = rocket_instance.launch().await?;
    

    Ok(())
}