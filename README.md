![Horizon Maestro Splash](branding/logo-no-background.png)

# Horizon Maestro

**Cluster Orchestrator for Horizon Game Server Infrastructure**

Horizon Maestro is a container orchestration layer that automatically deploys, manages, and scales [Horizon](https://github.com/Far-Beyond-Dev/Horizon) game servers and [Atlas](https://github.com/Far-Beyond-Dev/Horizon-Atlas) proxy instances. It provides a REST API for cluster management and integrates with Docker to handle the complete lifecycle of your distributed game server infrastructure.

## 🚀 Features

### Core Functionality
- **Auto-Bootstrap** - Automatically deploy Atlas + initial Horizon instances on startup
- **Dynamic Scaling** - Spawn new Horizon instances on-demand for new world regions
- **Container Orchestration** - Full Docker container lifecycle management via Bollard API
- **REST API** - Complete HTTP API for cluster control and monitoring
- **Health Monitoring** - Track instance health and automatically restart failed containers

### Integration
- **Horizon Game Servers** - Deploy and manage game server instances with proper configuration
- **Atlas Proxy** - Orchestrate the load-balancing proxy layer
- **Docker Native** - Direct Docker API integration (no compose required at runtime)
- **Shared Types** - Uses [Horizon-Network-Common](https://crates.io/crates/horizon-network-common) for API compatibility

## 🏗️ Architecture

Maestro sits above the infrastructure layer and orchestrates the entire cluster:

```
                    ┌─────────────────┐
                    │  Horizon Maestro │  ← Orchestrator (port 8000)
                    │    REST API      │
                    └────────┬────────┘
                             │ Docker API
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
        ┌──────────┐   ┌──────────┐   ┌──────────┐
        │  Atlas   │   │ Horizon  │   │ Horizon  │
        │  Proxy   │   │  (0,0,0) │   │  (1,0,0) │
        │ :9000/01 │   │  :8080   │   │  :8081   │
        └──────────┘   └──────────┘   └──────────┘
```

## 🚦 Getting Started

### Prerequisites
- Rust 1.70+
- Docker Desktop or Docker Engine running
- Horizon and Atlas Docker images available

### Quick Start

```bash
# Clone the repository
git clone https://github.com/Far-Beyond-Dev/Horizon-Maestro
cd Horizon-Maestro

# Run with auto-bootstrap enabled
MAESTRO_AUTO_BOOTSTRAP=true \
MAESTRO_HORIZON_CONFIG_PATH=/path/to/config.toml \
MAESTRO_ATLAS_IMAGE=ghcr.io/far-beyond-dev/horizon-atlas:main \
MAESTRO_HORIZON_IMAGE=ghcr.io/far-beyond-dev/horizon:main \
cargo run
```

Maestro will:
1. Start the REST API on port 8000
2. Deploy an Atlas proxy instance
3. Deploy initial Horizon game server(s) for region (0,0,0)
4. Begin health monitoring

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MAESTRO_AUTO_BOOTSTRAP` | `true` | Auto-deploy cluster on startup |
| `MAESTRO_ATLAS_IMAGE` | `ghcr.io/far-beyond-dev/horizon-atlas:main` | Atlas Docker image |
| `MAESTRO_HORIZON_IMAGE` | `ghcr.io/far-beyond-dev/horizon:main` | Horizon Docker image |
| `MAESTRO_HORIZON_CONFIG_PATH` | - | Host path to Horizon config.toml (mounted into containers) |
| `MAESTRO_HORIZON_BASE_PORT` | `8080` | Starting port for Horizon instances |
| `MAESTRO_REGION_SIZE` | `1000.0` | World units per region |

## 📡 REST API

### Cluster Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `GET /cluster/status` | GET | Get cluster status and all instances |
| `POST /cluster/bootstrap` | POST | Manually trigger cluster bootstrap |
| `POST /cluster/scale` | POST | Deploy a new region on-demand |
| `GET /cluster/instances` | GET | List all managed instances |

### Horizon Instances

| Endpoint | Method | Description |
|----------|--------|-------------|
| `GET /horizon/instances` | GET | List Horizon instances |
| `POST /horizon/instances` | POST | Create a new Horizon instance |
| `GET /horizon/instances/<id>` | GET | Get instance details |
| `PUT /horizon/instances/<id>/start` | PUT | Start an instance |
| `PUT /horizon/instances/<id>/stop` | PUT | Stop an instance |

### General Container Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `GET /instances` | GET | List all Docker containers |
| `POST /instances` | POST | Create a container |
| `GET /instances/<id>/logs` | GET | Get container logs |
| `GET /instances/<id>/stats` | GET | Get container stats |
| `DELETE /instances/<id>` | DELETE | Remove a container |

### Example: Scale a New Region

```bash
curl -X POST http://localhost:8000/cluster/scale \
  -H "Content-Type: application/json" \
  -d '{"region": {"x": 1, "y": 0, "z": 0}}'
```

## 🐳 Docker Deployment

### Using Docker Compose (Development)

```yaml
services:
  maestro:
    build: ./Horizon-Maestro
    ports:
      - "8000:8000"
    volumes:
      - /var/run/docker.sock:/var/run/docker.sock
      - ./Horizon/config.toml:/config/config.toml:ro
    environment:
      - MAESTRO_AUTO_BOOTSTRAP=false  # Services defined in compose
      - MAESTRO_HORIZON_CONFIG_PATH=/config/config.toml
```

### Standalone (Production)

When running Maestro standalone with `cargo run`, set `MAESTRO_AUTO_BOOTSTRAP=true` and it will dynamically create all containers via the Docker API.

## 🛠️ Development

### Project Structure
```
src/
├── main.rs           # Entry point, Rocket server setup
├── orchestrator.rs   # ClusterOrchestrator - core orchestration logic
├── agent.rs          # Agent identification
└── routes/
    ├── cluster_routes.rs   # /cluster/* endpoints
    ├── horizon_routes.rs   # /horizon/* endpoints  
    ├── instance_routes.rs  # /instances/* endpoints
    ├── image_routes.rs     # /images/* endpoints
    ├── network_routes.rs   # /networks/* endpoints
    └── volume_routes.rs    # /volumes/* endpoints
```

### Key Components

- **ClusterOrchestrator** - Manages the full cluster lifecycle, tracks deployed instances
- **ClusterConfig** - Configuration loaded from environment variables
- **AppManager** - Wrapper around Bollard Docker client

## 🤝 Integration with Horizon Ecosystem

Maestro is part of the Horizon distributed game server ecosystem:

- **[Horizon](https://github.com/Far-Beyond-Dev/Horizon)** - The game server instances
- **[Horizon-Atlas](https://github.com/Far-Beyond-Dev/Horizon-Atlas)** - WebSocket proxy and load balancer
- **[Horizon-Network-Common](https://crates.io/crates/horizon-network-common)** - Shared API types

## 📄 License

This project is licensed under the Apache 2.0 License - see the [LICENSE](LICENSE) file for details.

---

**Maestro** - Orchestrating distributed game server infrastructure.
