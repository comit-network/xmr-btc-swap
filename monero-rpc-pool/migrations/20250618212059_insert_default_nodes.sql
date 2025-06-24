-- Insert default mainnet bootstrap nodes
INSERT OR IGNORE INTO monero_nodes (scheme, host, port, full_url, network, first_seen_at) VALUES
    ('http', 'node.supportxmr.com', 18081, 'http://node.supportxmr.com:18081', 'mainnet', datetime('now')),
    ('http', 'nodes.hashvault.pro', 18081, 'http://nodes.hashvault.pro:18081', 'mainnet', datetime('now')),
    ('http', 'xmr-node.cakewallet.com', 18081, 'http://xmr-node.cakewallet.com:18081', 'mainnet', datetime('now')),
    ('http', 'node.xmr.to', 18081, 'http://node.xmr.to:18081', 'mainnet', datetime('now')),
    ('https', 'opennode.xmr-tw.org', 18089, 'https://opennode.xmr-tw.org:18089', 'mainnet', datetime('now')),
    ('https', 'monero.stackwallet.com', 18081, 'https://monero.stackwallet.com:18081', 'mainnet', datetime('now')),
    ('https', 'node.sethforprivacy.com', 18089, 'https://node.sethforprivacy.com:18089', 'mainnet', datetime('now')),
    ('https', 'node.monero.net', 18081, 'https://node.monero.net:18081', 'mainnet', datetime('now')),
    ('https', 'moneronode.org', 18081, 'https://moneronode.org:18081', 'mainnet', datetime('now')),
    ('http', 'node.majesticbank.at', 18089, 'http://node.majesticbank.at:18089', 'mainnet', datetime('now')),
    ('http', 'node.majesticbank.is', 18089, 'http://node.majesticbank.is:18089', 'mainnet', datetime('now')),
    ('https', 'xmr.cryptostorm.is', 18081, 'https://xmr.cryptostorm.is:18081', 'mainnet', datetime('now')),
    ('https', 'xmr.privex.io', 18081, 'https://xmr.privex.io:18081', 'mainnet', datetime('now')),
    ('https', 'nodes.hashvault.pro', 18081, 'https://nodes.hashvault.pro:18081', 'mainnet', datetime('now')),
    ('http', 'hashvaultsvg2rinvxz7kos77hdfm6zrd5yco3tx2yh2linsmusfwyad.onion', 18081, 'http://hashvaultsvg2rinvxz7kos77hdfm6zrd5yco3tx2yh2linsmusfwyad.onion:18081', 'mainnet', datetime('now')),
    ('https', 'plowsof3t5hogddwabaeiyrno25efmzfxyro2vligremt7sxpsclfaid.onion', 18089, 'https://plowsof3t5hogddwabaeiyrno25efmzfxyro2vligremt7sxpsclfaid.onion:18089', 'mainnet', datetime('now')),
    ('http', 'moneroexnovtlp4datcwbgjznnulgm7q34wcl6r4gcvccruhkceb2xyd.onion', 18089, 'http://moneroexnovtlp4datcwbgjznnulgm7q34wcl6r4gcvccruhkceb2xyd.onion:18089', 'mainnet', datetime('now')),
    ('https', 'yqz7oikk5fyxhyy32lyy3bkwcfw4rh2o5i77wuwslqll24g3bgd44iid.onion', 18081, 'https://yqz7oikk5fyxhyy32lyy3bkwcfw4rh2o5i77wuwslqll24g3bgd44iid.onion:18081', 'mainnet', datetime('now'));

-- Insert default stagenet bootstrap nodes
INSERT OR IGNORE INTO monero_nodes (scheme, host, port, full_url, network, first_seen_at) VALUES
    ('http', 'stagenet.xmr-tw.org', 38081, 'http://stagenet.xmr-tw.org:38081', 'stagenet', datetime('now')),
    ('https', 'node.monerodevs.org', 38089, 'https://node.monerodevs.org:38089', 'stagenet', datetime('now')),
    ('https', 'node2.monerodevs.org', 38089, 'https://node2.monerodevs.org:38089', 'stagenet', datetime('now')),
    ('https', 'node3.monerodevs.org', 38089, 'https://node3.monerodevs.org:38089', 'stagenet', datetime('now')),
    ('https', 'xmr-lux.boldsuck.org', 38081, 'https://xmr-lux.boldsuck.org:38081', 'stagenet', datetime('now')),
    ('http', 'plowsofe6cleftfmk2raiw5h2x66atrik3nja4bfd3zrfa2hdlgworad.onion', 38089, 'http://plowsofe6cleftfmk2raiw5h2x66atrik3nja4bfd3zrfa2hdlgworad.onion:38089', 'stagenet', datetime('now')),
    ('http', 'plowsoffjexmxalw73tkjmf422gq6575fc7vicuu4javzn2ynnte6tyd.onion', 38089, 'http://plowsoffjexmxalw73tkjmf422gq6575fc7vicuu4javzn2ynnte6tyd.onion:38089', 'stagenet', datetime('now')),
    ('https', 'stagenet.xmr.ditatompel.com', 38081, 'https://stagenet.xmr.ditatompel.com:38081', 'stagenet', datetime('now'));