-- This migration adds the xmr field to Bob's State3, State4, State5, and State6 across all relevant swap states.
-- The xmr value is copied from the earliest SwapSetupCompleted state (State2) within the same swap when available.

-- Bob: Add xmr to State3 inside BtcLocked
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcLocked.state3.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcLocked') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcLocked.state3.xmr') IS NULL;

-- Bob: Add xmr to State3 inside XmrLockProofReceived
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.XmrLockProofReceived.state.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.XmrLockProofReceived') IS NOT NULL
  AND json_extract(state, '$.Bob.XmrLockProofReceived.state.xmr') IS NULL;

-- Bob: Add xmr to State4 inside XmrLocked
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.XmrLocked.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.XmrLocked') IS NOT NULL
  AND json_extract(state, '$.Bob.XmrLocked.xmr') IS NULL;

-- Bob: Add xmr to State4 inside EncSigSent
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.EncSigSent.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.EncSigSent') IS NOT NULL
  AND json_extract(state, '$.Bob.EncSigSent.xmr') IS NULL;

-- Bob: Add xmr to State6 inside CancelTimelockExpired
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.CancelTimelockExpired.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.CancelTimelockExpired') IS NOT NULL
  AND json_extract(state, '$.Bob.CancelTimelockExpired.xmr') IS NULL;

-- Bob: Add xmr to State6 inside BtcCancelled
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcCancelled.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcCancelled') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcCancelled.xmr') IS NULL;

-- Bob: Add xmr to State6 inside BtcRefundPublished
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcRefundPublished.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcRefundPublished') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcRefundPublished.xmr') IS NULL;

-- Bob: Add xmr to State6 inside BtcEarlyRefundPublished
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcEarlyRefundPublished.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcEarlyRefundPublished') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcEarlyRefundPublished.xmr') IS NULL;

-- Bob: Add xmr to State6 inside BtcRefunded
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcRefunded.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcRefunded') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcRefunded.xmr') IS NULL;

-- Bob: Add xmr to State6 inside BtcEarlyRefunded
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcEarlyRefunded.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcEarlyRefunded') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcEarlyRefunded.xmr') IS NULL;

-- Bob: Add xmr to State6 inside BtcPunished.state
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcPunished.state.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcPunished') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcPunished.state.xmr') IS NULL;

-- Bob: Add xmr to State5 inside BtcRedeemed
UPDATE swap_states SET
    state = json_insert(
        state,
        '$.Bob.BtcRedeemed.xmr',
        (
            SELECT json_extract(states.state, '$.Bob.SwapSetupCompleted.xmr')
            FROM swap_states AS states
            WHERE
                states.swap_id = swap_states.swap_id
                AND json_extract(states.state, '$.Bob.SwapSetupCompleted') IS NOT NULL
            LIMIT 1
        )
    )
WHERE json_extract(state, '$.Bob.BtcRedeemed') IS NOT NULL
  AND json_extract(state, '$.Bob.BtcRedeemed.xmr') IS NULL; 