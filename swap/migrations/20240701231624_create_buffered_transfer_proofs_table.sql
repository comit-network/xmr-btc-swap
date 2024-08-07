CREATE TABLE if NOT EXISTS buffered_transfer_proofs
(
    swap_id     TEXT    PRIMARY KEY NOT NULL,
    proof       TEXT                NOT NULL
);