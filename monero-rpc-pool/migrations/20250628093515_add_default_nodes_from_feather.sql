-- Adds the default nodes from Feather Wallet to the database
-- Clears older nodes from the database

-- Delete all nodes from the database
DELETE FROM monero_nodes;

-- Delete all health checks
DELETE FROM health_checks;

-- Mainnet Nodes
INSERT OR IGNORE INTO monero_nodes (scheme, host, port, network, first_seen_at) VALUES
-- These support https
('https', 'node3-us.monero.love', 18081, 'mainnet', datetime('now')),
('https', 'xmr-node.cakewallet.com', 18081, 'mainnet', datetime('now')),
('https', 'node2.monerodevs.org', 18089, 'mainnet', datetime('now')),
('https', 'node3.monerodevs.org', 18089, 'mainnet', datetime('now')),
('https', 'node.sethforprivacy.com', 18089, 'mainnet', datetime('now')),
('https', 'xmr.stormycloud.org', 18089, 'mainnet', datetime('now')),
('https', 'node2-eu.monero.love', 18089, 'mainnet', datetime('now')),
('https', 'rucknium.me', 18081, 'mainnet', datetime('now')),
-- These do not support https
('http', 'singapore.node.xmr.pm', 18089, 'mainnet', datetime('now')),
('http', 'node.majesticbank.is', 18089, 'mainnet', datetime('now')),
('http', 'node.majesticbank.at', 18089, 'mainnet', datetime('now')),
('http', 'ravfx.its-a-node.org', 18081, 'mainnet', datetime('now')),
('http', 'ravfx2.its-a-node.org', 18089, 'mainnet', datetime('now')),
('http', 'selsta1.featherwallet.net', 18081, 'mainnet', datetime('now')),
('http', 'selsta2.featherwallet.net', 18081, 'mainnet', datetime('now')),
('http', 'node.trocador.app', 18089, 'mainnet', datetime('now')),
('http', 'node.xmr.ru', 18081, 'mainnet', datetime('now'));


-- Stagenet Nodes
INSERT OR IGNORE INTO monero_nodes (scheme, host, port, network, first_seen_at) VALUES
('https', 'node.sethforprivacy.com', 38089, 'stagenet', datetime('now')),
('https', 'xmr-lux.boldsuck.org', 38081, 'stagenet', datetime('now')),
('http', 'node2.sethforprivacy.com', 38089, 'stagenet', datetime('now')),
('http', 'stagenet.xmr-tw.org', 38081, 'stagenet', datetime('now')),
('http', 'singapore.node.xmr.pm', 38081, 'stagenet', datetime('now')),
('http', 'node.monerodevs.org', 38089, 'stagenet', datetime('now')),
('http', 'node2.monerodevs.org', 38089, 'stagenet', datetime('now')),
('http', 'node3.monerodevs.org', 38089, 'stagenet', datetime('now')),
('http', 'plowsoffjexmxalw73tkjmf422gq6575fc7vicuu4javzn2ynnte6tyd.onion', 38089, 'stagenet', datetime('now')),
('http', 'plowsof3t5hogddwabaeiyrno25efmzfxyro2vligremt7sxpsclfaid.onion', 38089, 'stagenet', datetime('now')),
('https', 'stagenet.xmr.ditatompel.com', 38081, 'stagenet', datetime('now'));