# Freebuff

**Open-source serverless PostgreSQL platform** — combining the best of [Neon](https://neon.tech) and [Supabase](https://supabase.com) into a single, self-hostable platform.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

## Features

- 🐘 **Serverless PostgreSQL** — Managed Postgres with branch-based workflows
- 🔀 **Database Branching** — Create instant branches for dev/staging/preview
- 🚀 **Auto-generated REST APIs** — PostgREST-compatible API from your schema
- 🔐 **Authentication** — Built-in auth with JWT, OAuth, and Row-Level Security
- ⚡ **Real-time** — WebSocket subscriptions for live data updates
- 📁 **Object Storage** — S3-compatible file storage
- 📊 **Dashboard** — Beautiful web UI for managing everything

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Next.js Dashboard                      │
└──────────────┬──────────────────────────┬───────────────┘
               │                          │
┌──────────────▼──────────┐  ┌────────────▼──────────────┐
│   Control Plane (Rust)   │  │    API Gateway (Rust)      │
│  - Project management    │  │  - REST auto-gen           │
│  - Branch management     │  │  - WebSocket real-time     │
│  - Compute lifecycle     │  │  - Auth middleware          │
└──────┬───────────────────┘  └───────┬───────────────────┘
       │                              │
┌──────▼──────────────────────────────▼───────────────────┐
│              Database Proxy (Rust)                        │
│  - Connection pooling    - Tenant routing                 │
│  - Branch routing        - Query logging                  │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│              PostgreSQL Instances (per-project)           │
└─────────────────────────────────────────────────────────┘
```

## Quick Start

### Prerequisites

- Rust 1.75+ (install via [rustup](https://rustup.rs))
- Node.js 18+ (for the dashboard)
- Docker & Docker Compose (for local dev dependencies)

### 1. Start Infrastructure

```bash
docker-compose -f docker/docker-compose.yml up -d
```

This starts:
- **PostgreSQL 16** on port `5432`
- **Redis** on port `6379`
- **MinIO** (S3-compatible) on ports `9000`/`9001`

### 2. Start the Control Plane

```bash
cargo run -p freebuff-control-plane
```

The control plane starts on `http://localhost:3001`.

### 3. Start the API Gateway

```bash
cargo run -p freebuff-gateway
```

The gateway starts on `http://localhost:8000`.

### 4. Start the Dashboard

```bash
cd dashboard
npm install
npm run dev
```

The dashboard starts on `http://localhost:3000`.

### 5. Create Your First Project

1. Open `http://localhost:3000`
2. Register an account
3. Click "New Project"
4. Your serverless PostgreSQL database is ready!

## API Reference

### Projects

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/v1/projects` | List all projects |
| `POST` | `/v1/projects` | Create a new project |
| `GET` | `/v1/projects/:id` | Get project details |
| `PUT` | `/v1/projects/:id` | Update a project |
| `DELETE` | `/v1/projects/:id` | Delete a project |

### Branches

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/v1/projects/:id/branches` | List branches |
| `POST` | `/v1/projects/:id/branches` | Create a branch |
| `GET` | `/v1/projects/:id/branches/:bid` | Get branch details |
| `DELETE` | `/v1/projects/:id/branches/:bid` | Delete a branch |

### REST API (PostgREST-compatible)

```bash
# List rows
curl -H "apikey: YOUR_KEY" http://localhost:8000/rest/v1/users

# Insert rows
curl -X POST \
  -H "apikey: YOUR_KEY" \
  -H "Content-Type: application/json" \
  -d '{"name": "John"}' \
  http://localhost:8000/rest/v1/users
```

## CLI Reference

Install and use the `freebuff` CLI to manage everything from the command line:

```bash
# Build and install the CLI
cargo install --path crates/cli

# Or run directly
cargo run -p freebuff-cli -- <command>
```

### Authentication

```bash
freebuff auth login          # Login to Freebuff
freebuff auth logout         # Logout
freebuff auth whoami         # Show current user
```

### Project Management

```bash
freebuff init                # Create a new project (interactive)
freebuff init --name my-db   # Create with specific name
freebuff projects            # List all projects
freebuff projects describe   # Show project details
freebuff delete my-project   # Delete a project
```

### Branch Management

```bash
freebuff branch list              # List branches
freebuff branch create dev        # Create branch from main
freebuff branch create staging --parent dev  # Create from specific parent
freebuff branch switch staging    # Switch default branch
freebuff branch delete old-branch # Delete a branch
freebuff branch diff              # Compare with parent
```

### Schema Management

```bash
freebuff push                    # Push SQL file to current branch
freebuff push --branch dev       # Push to specific branch
freebuff push --dry-run          # Preview changes without applying
freebuff diff                    # Compare schemas
freebuff diff main --to staging  # Compare specific branches
```

### Migrations

```bash
freebuff migrations create add_users   # Create new migration
freebuff migrations list               # List all migrations
freebuff migrations run                # Apply pending migrations
freebuff migrations run --dry-run      # Preview without applying
freebuff migrations rollback           # Rollback last migration
```

### Local Development

```bash
freebuff dev start              # Start local Postgres + Redis + MinIO
freebuff dev start --port 5433  # Start on custom port
freebuff dev stop               # Stop local environment
freebuff dev reset              # Reset database (destructive)
freebuff dev status             # Show running services
freebuff dev exec psql          # Run command in dev environment
```

### Connection Info

```bash
freebuff connect                    # Show connection info
freebuff connect --uri              # Output connection URI
freebuff connect --psql             # Output psql command
freebuff connect --branch staging   # Connect to specific branch
```

### Configuration

```bash
freebuff config show                # Show all config
freebuff config set api_url http://localhost:3001
freebuff config get default_branch
freebuff config reset               # Reset to defaults
```

### Diagnostics

```bash
freebuff doctor                     # Check system health
freebuff status                     # Show project status
freebuff status --watch 5           # Watch status (refresh every 5s)
freebuff --version                  # Show CLI version
```

## Project Structure

```
freebuff/
├── Cargo.toml                    # Rust workspace
├── crates/
│   ├── shared/                   # Shared types and utilities
│   ├── proxy/                    # Database proxy (connection pooling, routing)
│   ├── gateway/                  # API gateway (REST, WebSocket)
│   ├── auth/                     # Authentication service
│   ├── control-plane/            # Control plane API
│   ├── storage-service/          # Object storage
│   ├── wal-service/              # WAL reception, archiving, PITR
│   ├── branch-service/           # pg_basebackup branching
│   └── cli/                      # CLI tool (freebuff command)
├── dashboard/                    # Next.js dashboard
├── docker/                       # Docker Compose
├── migrations/                   # SQL migrations
└── docs/                         # Documentation
```

## Billing & Usage Metering (Stripe)

Freebuff meters **database storage (GB)**, **compute hours**, and **API calls** per organization, aggregates them daily, and reports them to [Stripe Billing Meters](https://docs.stripe.com/billing/usage-metering) for usage-based invoicing.

- **Compute hours** — accrued automatically by the control plane for every running compute endpoint (weighted by size: micro 0.25×, small 1×, medium 4×, large 16×).
- **API calls** — counted by the API gateway and batched to the control plane every 60s.
- **Database storage** — sampled from live `pg_database_size()` per active project (interval via `STORAGE_SAMPLE_SECS`).

### Setup

1. Set `STRIPE_SECRET_KEY` and `STRIPE_PRICE_ID` (a recurring price for the base plan) in the control plane's environment.
2. In the Stripe dashboard, create three **Billing Meters** named `freebuff_storage_gb`, `freebuff_compute_hours`, and `freebuff_api_calls` (or set custom names via `STRIPE_METER_*`), each mapping the customer via `payload.stripe_customer_id`, and attach them to metered prices on your subscription items.
3. Point a Stripe webhook at `POST /v1/billing/webhook` with `STRIPE_WEBHOOK_SECRET` set, subscribing to `checkout.session.completed`, `customer.subscription.*`, and `invoice.*` events.
4. Users manage plans from the dashboard's **Billing** page (checkout, billing portal, cancel).

Usage is visible per meter in `GET /v1/billing/usage` before it ever reaches Stripe, and services can report arbitrary usage to `POST /v1/internal/usage` (Bearer `INTERNAL_API_TOKEN`).

## Configuration

All services are configured via environment variables. See `.env.example` for the full list.

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `postgres://freebuff:freebuff@localhost:5432/freebuff` | Control plane database |
| `REDIS_URL` | `redis://localhost:6379` | Redis for caching |
| `JWT_SECRET` | `dev-secret` | JWT signing secret |
| `API_PORT` | `3000` | Control plane port |
| `GATEWAY_PORT` | `8000` | API gateway port |

## Roadmap

- [ ] **Phase 1** — Foundation ✅
- [ ] **Phase 2** — Developer Platform (auth, real-time, API gateway)
- [ ] **Phase 3** — Branching & Serverless (scale-to-zero, PITR)
- [ ] **Phase 4** — Production Hardening (K8s, observability, CLI)

## Contributing

Contributions welcome! Please read our contributing guidelines and open an issue or PR.

## License

Apache License 2.0
