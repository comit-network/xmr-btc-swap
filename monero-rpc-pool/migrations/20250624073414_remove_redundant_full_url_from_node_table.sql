-- Remove full_url column from monero_nodes table as it can be derived from scheme, host, and port

-- Drop the index first
DROP INDEX IF EXISTS idx_nodes_full_url;

-- Create a new table without the full_url column
CREATE TABLE monero_nodes_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scheme TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL,
    network TEXT NOT NULL,  -- mainnet/stagenet/testnet - always known at insertion time
    first_seen_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- Create a unique constraint on scheme, host, and port instead of full_url
    UNIQUE(scheme, host, port)
);

-- Copy data from old table to new table
INSERT INTO monero_nodes_new (id, scheme, host, port, network, first_seen_at, created_at, updated_at)
SELECT id, scheme, host, port, network, first_seen_at, created_at, updated_at
FROM monero_nodes;

-- Drop the old table
DROP TABLE monero_nodes;

-- Rename the new table to the original name
ALTER TABLE monero_nodes_new RENAME TO monero_nodes;

-- Recreate the indexes (excluding the full_url index)
CREATE INDEX IF NOT EXISTS idx_nodes_network ON monero_nodes(network);
CREATE INDEX IF NOT EXISTS idx_nodes_scheme_host_port ON monero_nodes(scheme, host, port); 