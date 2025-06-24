-- Fix monerodevs.org nodes: change from https to http
UPDATE monero_nodes 
SET scheme = 'http' 
WHERE host = 'node.monerodevs.org' AND network = 'stagenet';

UPDATE monero_nodes 
SET scheme = 'http' 
WHERE host = 'node2.monerodevs.org' AND network = 'stagenet';

UPDATE monero_nodes 
SET scheme = 'http' 
WHERE host = 'node3.monerodevs.org' AND network = 'stagenet';