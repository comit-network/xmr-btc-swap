UPDATE swap_states SET state = json_replace(
    state, 
    '$.Alice.Done', 
    json_object(
      'BtcPunished', 
      (
        SELECT json_extract ( state, '$.Alice.BtcLocked' )  FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Alice.BtcLocked' ) IS NOT NULL
      )
    )
  ) 
WHERE json_extract ( state, '$.Alice.Done' ) = 'BtcPunished';
UPDATE swap_states SET state = json_replace (
    state, 
    '$.Bob', 
    json_object(
      'BtcPunished',
      json_object (
        'state',
      json_insert (
        (
          SELECT json_extract ( state, '$.Bob.BtcCancelled' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcCancelled' ) IS NOT NULL
        ),
        '$.v', 
        (
          SELECT json_extract ( state, '$.Bob.BtcLocked.state3.v' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
        ), 
        '$.monero_wallet_restore_blockheight', 
        (
          SELECT json_extract ( state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
        )
      ),
    'tx_lock_id', 
    json_extract (
        state, '$.Bob.Done.BtcPunished.tx_lock_id'
    )
    )
  )
  )
WHERE json_extract( state, '$.Bob.Done.BtcPunished' ) IS NOT NULL;
UPDATE swap_states SET state = json_insert (
    state, 
    '$.v', 
    (
      SELECT json_extract ( state, '$.Bob.BtcLocked.state3.v' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract( state, '$.Bob.BtcLocked' ) IS NOT NULL
    ), 
    '$.monero_wallet_restore_blockheight', 
    (
      SELECT json_extract ( state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
    )
  ) 
WHERE json_extract(state, '$.Bob.Done.BtcRefunded') IS NOT NULL;
UPDATE swap_states SET state = json_insert (
    state, 
    '$.v', 
    (
      SELECT json_extract ( state, '$.Bob.BtcLocked.state3.v' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
    ), 
    '$.monero_wallet_restore_blockheight', 
    (
      SELECT json_extract ( state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
    )
  ) 
WHERE json_extract( state, '$.Bob.BtcCancelled' ) IS NOT NULL;
UPDATE swap_states SET state = json_insert(
    state, 
    '$.v', 
    (
      SELECT json_extract ( state, '$.Bob.BtcLocked.state3.v' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
    ), 
    '$.monero_wallet_restore_blockheight', 
    (
      SELECT json_extract ( state, '$.Bob.BtcLocked.monero_wallet_restore_blockheight' ) FROM swap_states states WHERE states.swap_id = swap_states.swap_id AND json_extract ( state, '$.Bob.BtcLocked' ) IS NOT NULL
    )
  ) 
WHERE json_extract ( state, '$.Bob.CancelTimelockExpired' ) IS NOT NULL;