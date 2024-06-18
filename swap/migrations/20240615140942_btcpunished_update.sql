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
                    AND json_extract(
                        states.state, '$.Alice.BtcLocked'
                    ) IS NOT NULL -- Filters out only the BtcLocked state. (json_extract returns null if property doesn't exist)
            )
        )
    )
WHERE json_extract(state, '$.Alice.Done') = 'BtcPunished'; -- Apply update only to BtcPunished state rows
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
                        SELECT json_extract(states.state, '$.Bob.BtcCancelled') -- Get state6 from BtcCancelled state.
                        FROM swap_states AS states
                        WHERE
                            states.swap_id = swap_states.swap_id
                            AND json_extract(
                                states.state, '$.Bob.BtcCancelled'
                            ) IS NOT NULL -- Filters out only the BtcCancelled state. (json_extract returns null if property doesn't exist)
                    ),
                    '$.v', -- {"Bob":{"BtcPunished":{"state": {..., "v": "..."}, "tx_lock_id": "..."}}}
                    ( -- We get v property from BtcLocked state
                        SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v') -- Get v property from BtcLocked state
                        FROM swap_states AS states
                        WHERE
                            states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcPunished row
                            AND json_extract(
                                states.state, '$.Bob.BtcLocked'
                            ) IS NOT NULL -- Filters out only the BtcLocked state. (json_extract returns null if property doesn't exist)
                    ),
                    '$.monero_wallet_restore_blockheight', -- { "Bob": { "BtcPunished":{"state": {..., "monero_wallet_restore_blockheight": {"height":...}} }, "tx_lock_id": "..."} } }
                    ( -- We get monero_wallet_restore_blockheight property from BtcLocked state
                        SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
                        FROM swap_states AS states
                        WHERE
                            states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcPunished row, states.swap_id is id of the row that we are looking for
                            AND json_extract(
                                states.state, '$.Bob.BtcLocked'
                            ) IS NOT NULL  -- Filters out only the BtcLocked state. (json_extract returns null if property doesn't exist)
                    )
                ),
                'tx_lock_id', -- Insert tx_lock_id BtcPunished -> {"Bob": {"Done": {"BtcPunished": {"state":{<state object>}, "tx_lock_id": "..."} } }
                json_extract( -- Gets tx_lock_id from original state row
                    state, '$.Bob.Done.BtcPunished.tx_lock_id'
                )
            )
        )
    )
WHERE json_extract(state, '$.Bob.Done.BtcPunished') IS NOT NULL; -- Apply update only to BtcPunished state rows (json_extract returns null if property doesn't exist)
UPDATE swap_states SET
    state = json_insert(
        state, -- object that we insert properties into (original state from the row)
        '$.v', -- {"Bob":{"BtcRefunded":{..., "v": "..."}}
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id -- swap_states.swap_id is id of the BtcRefunded row, states.swap_id is id of the row that we are looking for
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        ),
        '$.monero_wallet_restore_blockheight',  -- {"Bob":{"BtcRefunded":{..., "monero_wallet_restore_blockheight": {"height":...}} }}
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        )
    )
WHERE json_extract(state, '$.Bob.Done.BtcRefunded') IS NOT NULL; -- Apply update only to BtcRefunded state rows (json_extract returns null if property doesn't exist)
UPDATE swap_states SET -- Copy of previous query
    state = json_insert(
        state,
        '$.v',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        ),
        '$.monero_wallet_restore_blockheight',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        )
    )
WHERE json_extract(state, '$.Bob.BtcCancelled') IS NOT NULL;
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.v',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.state3.v')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        ),
        '$.monero_wallet_restore_blockheight',
        (
            SELECT json_extract(states.state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.BtcLocked') IS NOT NULL
        )
    )
WHERE json_extract(state, '$.Bob.CancelTimelockExpired') IS NOT NULL;