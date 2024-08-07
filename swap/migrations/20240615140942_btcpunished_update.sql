-- This migration script modifies swap states to be compatible with the new state structure introduced in PR #1676.
-- The following changes are made:
-- 1. Bob: BtcPunished state now has a new attribute 'state' (type: State6), 'tx_lock_id' (type: string) remains the same
-- 2. Bob: State6 has two new attributes: 'v' (monero viewkey) and 'monero_wallet_restore_blockheight' (type: BlockHeight)
--    State6 is used in BtcPunished, CancelTimelockExpired, BtcCancelled, BtcRefunded states
-- 3. Alice: BtcPunished state now has a new attribute 'state3' (type: State3)

-- Alice: Add new attribute 'state3' (type: State3) to the BtcPunished state by copying it from the BtcLocked state
UPDATE swap_states SET
    state = json_replace( -- Replaces "{"Alice":{"Done":"BtcPunished"}}" with "{"Alice": {"Done": "BtcPunished": {"state": <state3 object from BtcLocked>} }}"
        state,
        '$.Alice.Done',
        json_object(
            'BtcPunished',
            (
                SELECT json_extract(states.state, '$.Alice.BtcLocked') -- Read state3 object from BtcLocked
                FROM swap_states AS states
                WHERE
                    states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcPunished row
                    AND json_extract(states.state, '$.Alice.BtcLocked') IS NOT NULL -- Filters out only the BtcLocked state
            )
        )
    )
WHERE json_extract(state, '$.Alice.Done') = 'BtcPunished'; -- Apply update only to BtcPunished state rows

-- Bob: Add new attribute 'state6' (type: State6) to the BtcPunished state by copying it from the BtcCancelled state
-- and add new State6 attributes 'v' and 'monero_wallet_restore_blockheight' from the BtcLocked state
UPDATE swap_states SET
    state = json_replace(
        state,
        '$.Bob',  -- Replace '{"Bob":{"Done": {"BtcPunished": {"tx_lock_id":"..."} }}}' with {"Bob":{"BtcPunished":{"state":{<state6 object>}, "tx_lock_id": "..."}}}
        json_object(
            'BtcPunished', -- {"Bob":{"BtcPunished":{}}
            json_object(
                'state', -- {"Bob":{"BtcPunished":{"state": {}}}
                json_insert(
                    ( -- object that we insert properties into (original state6 from BtcCancelled state)
                        SELECT json_extract(states.state, '$.Bob.BtcCancelled') -- Get state6 from BtcCancelled state
                        FROM swap_states AS states
                        WHERE
                            states.swap_id = swap_states.swap_id
                            AND json_extract(states.state, '$.Bob.BtcCancelled') IS NOT NULL -- Filters out only the BtcCancelled state
                    ),
                    '$.v', -- {"Bob":{"BtcPunished":{"state": {..., "v": "..."}, "tx_lock_id": "..."}}}
                    ( -- Get v property from BtcLocked state
                        SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v') 
                        FROM swap_states AS states
                        WHERE
                            states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcPunished row
                            AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL -- Filters out only the BtcLocked state
                    ),
                    '$.monero_wallet_restore_blockheight', -- { "Bob": { "BtcPunished":{"state": {..., "monero_wallet_restore_blockheight": {"height":...}} }, "tx_lock_id": "..."} } }
                    ( -- Get monero_wallet_restore_blockheight property from BtcLocked state
                        SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
                        FROM swap_states AS states
                        WHERE 
                            states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcPunished row, states.swap_id is id of the row that we are looking for
                            AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL  -- Filters out only the BtcLocked state
                    )  
                ),
                'tx_lock_id', -- Insert tx_lock_id BtcPunished -> {"Bob": {"Done": {"BtcPunished": {"state":{<state object>}, "tx_lock_id": "..."} } }
                json_extract(state, '$.Bob.Done.BtcPunished.tx_lock_id') -- Gets tx_lock_id from original state row
            )
        )  
    )
WHERE json_extract(state, '$.Bob.Done.BtcPunished') IS NOT NULL; -- Apply update only to BtcPunished state rows

-- Bob: Add new State6 attributes 'v' and 'monero_wallet_restore_blockheight' to the BtcRefunded state
UPDATE swap_states SET
    state = json_insert(
        state, -- Object that we insert properties into (original state from the row) 
        '$.Bob.Done.BtcRefunded.v', -- {"Bob":{"BtcRefunded":{..., "v": "..."}}}
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcRefunded row, states.swap_id is id of the row that we are looking for
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        ),
        '$.Bob.Done.BtcRefunded.monero_wallet_restore_blockheight',  -- {"Bob":{"BtcRefunded":{..., "monero_wallet_restore_blockheight": {"height":...}} }}
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        )
    )
WHERE json_extract(state, '$.Bob.Done.BtcRefunded') IS NOT NULL; -- Apply update only to BtcRefunded state rows

-- Bob: Add new State6 attributes 'v' and 'monero_wallet_restore_blockheight' to the BtcCancelled state
UPDATE swap_states SET 
    state = json_insert(
        state,
        '$.Bob.BtcCancelled.v',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id  
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        ),
        '$.Bob.BtcCancelled.monero_wallet_restore_blockheight',
        (    
            SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
            FROM swap_states AS states
            WHERE 
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        )
    )
WHERE json_extract(state, '$.Bob.BtcCancelled') IS NOT NULL; -- Apply update only to BtcCancelled state rows
        
-- Bob: Add new State6 attributes 'v' and 'monero_wallet_restore_blockheight' to the CancelTimelockExpired state        
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.CancelTimelockExpired.v',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        ),
        '$.Bob.CancelTimelockExpired.monero_wallet_restore_blockheight',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')        
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        )
    )
WHERE json_extract(state, '$.Bob.CancelTimelockExpired') IS NOT NULL; -- Apply update only to CancelTimelockExpired state rows