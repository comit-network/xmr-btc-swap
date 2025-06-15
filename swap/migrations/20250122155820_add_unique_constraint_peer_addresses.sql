-- SQLite doesn't support adding constraints via ALTER TABLE
-- We need to recreate the table with the constraint
CREATE TABLE peer_addresses_new (
    peer_id TEXT NOT NULL,
    address TEXT NOT NULL,
    UNIQUE(peer_id, address)
);

-- Copy existing data, ensuring only unique combinations are inserted
INSERT INTO peer_addresses_new 
SELECT DISTINCT peer_id, address 
FROM peer_addresses;

-- Drop the old table
DROP TABLE peer_addresses;

-- Rename the new table to the original name
ALTER TABLE peer_addresses_new RENAME TO peer_addresses;