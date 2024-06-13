CREATE TABLE if NOT EXISTS swap_states
(
    id          INTEGER PRIMARY KEY autoincrement NOT NULL,
    swap_id     TEXT                NOT NULL,
    entered_at  TEXT                NOT NULL,
    state       TEXT                NOT NULL
);

CREATE TABLE if NOT EXISTS monero_addresses
(
    swap_id     TEXT    PRIMARY KEY NOT NULL,
    address     TEXT                NOT NULL
);

CREATE TABLE if NOT EXISTS peers
(
    swap_id     TEXT    PRIMARY KEY NOT NULL,
    peer_id     TEXT                NOT NULL
);

CREATE TABLE if NOT EXISTS peer_addresses
(
    peer_id     TEXT                NOT NULL,
    address     TEXT                NOT NULL
);