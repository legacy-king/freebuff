-- Freebuff Control Plane Database Schema

-- ── Custom Types ────────────────────────────────────────────────────────

CREATE TYPE project_status AS ENUM (
    'creating', 'active', 'suspended', 'deleting', 'deleted', 'failed'
);

CREATE TYPE branch_status AS ENUM (
    'creating', 'active', 'inactive', 'deleting', 'failed'
);

CREATE TYPE compute_status AS ENUM (
    'starting', 'running', 'stopping', 'stopped', 'failed'
);

CREATE TYPE compute_size AS ENUM (
    'micro', 'small', 'medium', 'large'
);

CREATE TYPE api_key_type AS ENUM (
    'publishable', 'secret'
);

-- ── Organizations ───────────────────────────────────────────────────────

CREATE TABLE organizations (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── Users ───────────────────────────────────────────────────────────────

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255),
    password_hash VARCHAR(255) NOT NULL,
    email_verified BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_email ON users(email);

-- ── Organization Memberships ────────────────────────────────────────────

CREATE TABLE org_members (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(50) NOT NULL DEFAULT 'member',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(org_id, user_id)
);

-- ── Projects ────────────────────────────────────────────────────────────

CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) NOT NULL,
    region VARCHAR(50) NOT NULL DEFAULT 'us-east-1',
    status project_status NOT NULL DEFAULT 'creating',
    database_host VARCHAR(255),
    database_port INTEGER DEFAULT 5432,
    database_name VARCHAR(255),
    api_url VARCHAR(500),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(org_id, slug)
);

CREATE INDEX idx_projects_org ON projects(org_id);
CREATE INDEX idx_projects_status ON projects(status);

-- ── Branches ────────────────────────────────────────────────────────────

CREATE TABLE branches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    slug VARCHAR(100) NOT NULL,
    parent_branch_id UUID REFERENCES branches(id) ON DELETE SET NULL,
    parent_lsn BIGINT,
    status branch_status NOT NULL DEFAULT 'creating',
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, slug)
);

CREATE INDEX idx_branches_project ON branches(project_id);

-- ── Compute Endpoints ───────────────────────────────────────────────────

CREATE TABLE compute_endpoints (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_id UUID NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    host VARCHAR(255) NOT NULL,
    port INTEGER NOT NULL DEFAULT 5432,
    status compute_status NOT NULL DEFAULT 'starting',
    compute_size compute_size NOT NULL DEFAULT 'small',
    max_connections INTEGER NOT NULL DEFAULT 100,
    -- Last time this endpoint's running time was accrued as a usage event (for compute-hours metering)
    last_accrued_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_compute_endpoints_branch ON compute_endpoints(branch_id);
CREATE INDEX idx_compute_endpoints_project ON compute_endpoints(project_id);

-- ── Roles ───────────────────────────────────────────────────────────────

CREATE TABLE roles (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    branch_id UUID NOT NULL REFERENCES branches(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    is_superuser BOOLEAN NOT NULL DEFAULT FALSE,
    can_login BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(branch_id, name)
);

-- ── API Keys ────────────────────────────────────────────────────────────

CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    key_hash VARCHAR(255) NOT NULL,
    key_prefix VARCHAR(20) NOT NULL,
    key_type api_key_type NOT NULL DEFAULT 'publishable',
    scopes TEXT[] NOT NULL DEFAULT '{}',
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ
);

CREATE INDEX idx_api_keys_project ON api_keys(project_id);
CREATE INDEX idx_api_keys_prefix ON api_keys(key_prefix);

-- ── Audit Log ───────────────────────────────────────────────────────────

CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID REFERENCES organizations(id) ON DELETE SET NULL,
    user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    action VARCHAR(100) NOT NULL,
    resource_type VARCHAR(100) NOT NULL,
    resource_id UUID,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_audit_log_org ON audit_log(org_id, created_at DESC);
CREATE INDEX idx_audit_log_project ON audit_log(project_id, created_at DESC);

-- ── Helper Functions ────────────────────────────────────────────────────

CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_organizations_updated_at
    BEFORE UPDATE ON organizations
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER update_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER update_projects_updated_at
    BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER update_branches_updated_at
    BEFORE UPDATE ON branches
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER update_compute_endpoints_updated_at
    BEFORE UPDATE ON compute_endpoints
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- ── Billing & Usage Metering ──────────────────────────────────────────────

CREATE TYPE usage_meter AS ENUM (
    'storage_gb', 'compute_hours', 'api_calls'
);

-- One billing account per organization, linked to a Stripe customer.
CREATE TABLE billing_accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL UNIQUE REFERENCES organizations(id) ON DELETE CASCADE,
    stripe_customer_id VARCHAR(255),
    stripe_subscription_id VARCHAR(255),
    plan VARCHAR(50) NOT NULL DEFAULT 'free',
    status VARCHAR(50) NOT NULL DEFAULT 'free',
    billing_email VARCHAR(255),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Raw metering events. Aggregated into usage_daily by the rollup task,
-- then submitted to Stripe Billing Meters by the reporter task.
CREATE TABLE usage_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    meter usage_meter NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    rolled_up BOOLEAN NOT NULL DEFAULT FALSE,
    idempotency_key VARCHAR(255) UNIQUE
);

CREATE INDEX idx_usage_events_org_time ON usage_events(org_id, occurred_at DESC);
CREATE INDEX idx_usage_events_pending ON usage_events(rolled_up) WHERE rolled_up = FALSE;

-- Daily aggregates per organization per meter. `submitted` tracks whether
-- the value has been reported to Stripe.
CREATE TABLE usage_daily (
    org_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    meter usage_meter NOT NULL,
    window_start DATE NOT NULL,
    value DOUBLE PRECISION NOT NULL DEFAULT 0,
    submitted BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (org_id, meter, window_start)
);

CREATE INDEX idx_usage_daily_pending ON usage_daily(submitted) WHERE submitted = FALSE;

-- Idempotency ledger for Stripe webhooks.
CREATE TABLE stripe_webhook_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    stripe_event_id VARCHAR(255) NOT NULL UNIQUE,
    event_type VARCHAR(100) NOT NULL,
    payload JSONB NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TRIGGER update_billing_accounts_updated_at
    BEFORE UPDATE ON billing_accounts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- ── Default Organization ────────────────────────────────────────────────

INSERT INTO organizations (id, name, slug) VALUES
    ('00000000-0000-0000-0000-000000000001', 'Freebuff', 'freebuff')
ON CONFLICT DO NOTHING;
