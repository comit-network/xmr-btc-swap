-- Add migration script here

-- Create monero_nodes table - stores node identity and current state
CREATE TABLE IF NOT EXISTS monero_nodes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scheme TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    full_url TEXT NOT NULL UNIQUE,
    network TEXT NOT NULL,  -- mainnet/stagenet/testnet - always known at insertion time
    first_seen_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Create health_checks table - stores raw event data
CREATE TABLE IF NOT EXISTS health_checks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id INTEGER NOT NULL,
    timestamp TEXT NOT NULL,
    was_successful BOOLEAN NOT NULL,
    latency_ms REAL,
    FOREIGN KEY (node_id) REFERENCES monero_nodes(id) ON DELETE CASCADE
);

-- Create indexes for performance
CREATE INDEX IF NOT EXISTS idx_nodes_full_url ON monero_nodes(full_url);
CREATE INDEX IF NOT EXISTS idx_nodes_network ON monero_nodes(network);
CREATE INDEX IF NOT EXISTS idx_health_checks_node_id ON health_checks(node_id);
CREATE INDEX IF NOT EXISTS idx_health_checks_timestamp ON health_checks(timestamp);
