# Branching Architecture

## Overview

Freebuff implements Neon-inspired database branching using PostgreSQL's native capabilities. This document describes the architecture and data flow for WAL streaming, branch creation, and Point-in-Time Recovery (PITR).

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                    Branch Service                                │
│  - pg_basebackup for branch creation                            │
│  - Postgres instance lifecycle management                       │
└──────────────┬──────────────────────────────────┬───────────────┘
               │                                  │
┌──────────────▼──────────┐  ┌────────────────────▼──────────────┐
│   Control Plane (Rust)   │  │        PostgreSQL Instance         │
│  - Branch metadata       │  │  - Primary (main branch)          │
│  - LSN tracking          │  │  - Branches (copy-on-write)       │
│  - Timeline management   │  │  - WAL streaming enabled          │
└──────────┬───────────────┘  └──────────────┬───────────────────┘
           │                                  │
┌──────────▼───────────────┐  ┌──────────────▼───────────────────┐
│      WAL Service (Rust)   │  │        S3/MinIO Storage           │
│  - WAL reception          │  │  - WAL segment archives           │
│  - Timeline management    │  │  - Base backups                   │
│  - WAL serving            │  │  - Branch snapshots               │
│  - PITR restoration       │  │                                   │
└───────────────────────────┘  └───────────────────────────────────┘
```

## Core Concepts

### Log Sequence Number (LSN)

An LSN is a monotonic integer that represents a specific point in PostgreSQL's WAL history. It's the foundation for all branching operations.

```
LSN Format: XX/XXXXXXXX (e.g., "0/15D6B38")
Raw value:  (hi << 32) | lo
```

### Timeline

A timeline represents the WAL history of a single PostgreSQL instance. Each branch gets its own timeline that shares history with its parent up to the fork point.

### WAL Segment

WAL data is organized into 16MB segments. Each segment is identified by its starting LSN:

```
Segment ID = LSN / (16 * 1024 * 1024)
```

## Branch Creation Flow

### 1. Create Branch Request

When a user creates a branch, the control plane:

```rust
POST /v1/projects/:project_id/branches
{
    "name": "feature-auth",
    "parent_branch_id": "optional-parent-id",
    "parent_lsn": "optional-target-lsn"
}
```

### 2. WAL Timeline Creation

The WAL service creates a new timeline:

```
POST /v1/timelines
{
    "timeline_id": "<branch-id>",
    "parent_id": "<parent-timeline-id>",
    "parent_branch_lsn": "0/15D6B38"  // fork point
}
```

### 3. Physical Branch Creation

The branch service uses `pg_basebackup`:

```bash
pg_basebackup \
    -h localhost \
    -p 5432 \
    -D /data/branches/<branch-id> \
    -U postgres \
    -F p \           # Plain format
    -X stream \      # Stream WAL during backup
    -P \             # Show progress
    --checkpoint=fast
```

### 4. Configuration

The branched instance gets unique settings:

```ini
# postgresql.conf for branch
listen_addresses = 'localhost'
port = 5400  # Unique per branch
wal_level = replica
max_wal_senders = 5
primary_conninfo = 'host=localhost port=5432'
```

## WAL Streaming

### Primary → WAL Service

PostgreSQL streams WAL to the WAL service in real-time:

```
┌─────────────┐     WAL Records     ┌─────────────┐
│  PostgreSQL  │ ──────────────────> │ WAL Service  │
│   Primary    │                     │  (Receiver)  │
└─────────────┘                     └──────┬──────┘
                                           │
                                           ▼
                                    ┌─────────────┐
                                    │   S3/MinIO   │
                                    │  (Archived)  │
                                    └─────────────┘
```

### WAL Service API

```bash
# Receive WAL from primary
POST /v1/timelines/:timeline_id/wal
Body: <binary WAL data>

# Serve WAL to replicas
GET /v1/timelines/:timeline_id/wal/:start_lsn
Query: max_bytes=1048576&timeout_ms=1000

# Archive WAL segments
POST /v1/timelines/:timeline_id/archive
```

## Point-in-Time Recovery (PITR)

### Restore Flow

```
1. User requests PITR to target LSN/timestamp
2. WAL service lists archived segments
3. Download segments from S3
4. Decompress and replay WAL
5. Configure Postgres recovery settings
6. Start Postgres in recovery mode
```

### PITR API

```bash
POST /v1/branches/:branch_id/restore
{
    "target_lsn": "0/15D6B38",
    "target_time": "2024-01-15T10:30:00Z"
}
```

### Recovery Configuration

The branch service writes recovery settings to `postgresql.auto.conf`:

```ini
restore_command = 'echo recovery not needed for %f'
recovery_target_lsn = '0/15D6B38'
recovery_target_action = 'promote'
```

And creates `standby.signal` to trigger recovery mode.

## WAL Archiving

### Archive Format

WAL segments are compressed with zstd and uploaded to S3:

```
Key format: {timeline_id}/{segment_id:016x}.wal.zst

Example: abc123/0000000000000001.wal.zst
```

### Archive Process

```rust
// Compress with zstd (level 3)
let compressed = zstd::encode_all(&segment.data, 3)?;

// Upload to S3
client.put(&s3_url)
    .basic_auth(&access_key, Some(&secret_key))
    .body(compressed)
    .send()
    .await?;
```

### Restore Process

```rust
// Download from S3
let compressed = client.get(&s3_url).send().await?.bytes().await?;

// Decompress
let wal_data = zstd::decode_all(compressed.as_ref())?;

// Apply to Postgres
```

## Data Structures

### Timeline

```rust
pub struct Timeline {
    pub id: String,
    pub parent_id: Option<String>,
    pub parent_branch_lsn: Option<LSN>,
    pub state: TimelineState,
    pub segments: BTreeMap<u64, WalSegment>,
    pub current_lsn: LSN,
    pub write_notify: Notify,
    pub data_dir: PathBuf,
}
```

### LSN

```rust
pub struct LSN(pub u64);

impl LSN {
    pub fn from_str(s: &str) -> Result<Self, String>;
    pub fn to_pg_string(&self) -> String;  // "0/15D6B38"
}
```

## Performance Characteristics

| Operation | Latency | Notes |
|-----------|---------|-------|
| Branch creation | ~1-5s | pg_basebackup with hard links |
| WAL streaming | <1ms | In-memory append |
| WAL archival | ~100ms | Compressed upload to S3 |
| PITR restore | ~5-30s | Depends on WAL volume |
| WAL serving | <1ms | Memory-first, S3 fallback |

## Production Considerations

### Scalability

- WAL segments are stored in-memory with lazy archival
- Branches share parent WAL up to fork point (copy-on-write)
- Object storage provides unlimited history retention

### Durability

- WAL quorum writes (3 safekeepers in Neon)
- Cross-AZ replication for WAL archives
- Base backups stored redundantly

### Security

- TLS for all WAL streaming connections
- Encrypted WAL archives in S3
- Access control via IAM policies

## Comparison with Neon

| Feature | Freebuff | Neon |
|---------|----------|------|
| Branching | pg_basebackup + WAL | Custom storage engine |
| WAL Storage | S3/MinIO | Custom safekeepers |
| PITR | WAL replay | Layer files + WAL |
| Scale to Zero | pg_ctl stop | Compute autoscaling |
| Copy-on-Write | File-level | Page-level |

Freebuff uses PostgreSQL's native capabilities for simplicity, while Neon reimplements the storage layer for maximum efficiency. Both approaches achieve the same developer experience.
