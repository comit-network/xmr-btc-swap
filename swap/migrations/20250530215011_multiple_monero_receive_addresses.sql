-- The user can now have multiple monero receive addresses
-- for a single swap
-- Each address has a percentage (0 to 1) of the amount they'll receive of the total of the swap amount
-- The sum of the percentages must for a single swap MUST be 1
-- Add percentage column with default value of 1.0
ALTER TABLE monero_addresses ADD COLUMN percentage REAL NOT NULL DEFAULT 1.0;

-- SQLite doesn't support dropping PRIMARY KEY constraint directly
-- We need to recreate the table without the PRIMARY KEY on swap_id
CREATE TABLE monero_addresses_temp
(
    swap_id     TEXT                NOT NULL,
    address     TEXT                NOT NULL,
    percentage  REAL                NOT NULL DEFAULT 1.0,
    label       TEXT                NOT NULL DEFAULT 'user address'
);

-- Copy data from the original table
INSERT INTO monero_addresses_temp (swap_id, address, percentage)
SELECT swap_id, address, percentage FROM monero_addresses;

-- Drop the original table
DROP TABLE monero_addresses;

-- Rename the temporary table
ALTER TABLE monero_addresses_temp RENAME TO monero_addresses;

-- Create an index on swap_id for performance
CREATE INDEX idx_monero_addresses_swap_id ON monero_addresses(swap_id);