-- Users don't have to specify a receive address for the swap anymore, if none is present
-- we will use the internal wallet address instead.
-- Now, the monero_addresses.address column can be NULL.

-- SQLite doesn't support MODIFY COLUMN directly
-- We need to recreate the table with the desired schema
CREATE TABLE monero_addresses_temp
(
    swap_id     TEXT                NOT NULL,
    address     TEXT                NULL,
    percentage  REAL                NOT NULL DEFAULT 1.0,
    label       TEXT                NOT NULL DEFAULT 'user address'
);

-- Copy data from the original table
INSERT INTO monero_addresses_temp (swap_id, address, percentage, label)
SELECT swap_id, address, percentage, label FROM monero_addresses;

-- Drop the original table
DROP TABLE monero_addresses;

-- Rename the temporary table
ALTER TABLE monero_addresses_temp RENAME TO monero_addresses;

-- Create an index on swap_id for performance
CREATE INDEX idx_monero_addresses_swap_id ON monero_addresses(swap_id);
