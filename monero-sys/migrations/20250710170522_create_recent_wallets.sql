-- Add migration script here

CREATE TABLE recent_wallets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    wallet_path TEXT UNIQUE NOT NULL,
    last_opened_at TEXT NOT NULL
);

CREATE INDEX idx_recent_wallets_last_opened ON recent_wallets(last_opened_at DESC);
