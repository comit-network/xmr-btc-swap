import { listen } from "@tauri-apps/api/event";
import { TauriSwapProgressEventWrapper, TauriContextStatusEvent, TauriLogEvent, BalanceResponse, TauriDatabaseStateEvent, TauriTimelockChangeEvent, TauriBackgroundRefundEvent, ApprovalRequest } from "models/tauriModel";
import { contextStatusEventReceived, receivedCliLog, rpcSetBalance, timelockChangeEventReceived, rpcSetBackgroundRefundState, approvalEventReceived } from "store/features/rpcSlice";
import { swapProgressEventReceived } from "store/features/swapSlice";
import logger from "utils/logger";
import { updatePublicRegistry, updateRates } from "./api";
import { checkContextAvailability, getSwapInfo, initializeContext, updateAllNodeStatuses } from "./rpc";
import { store } from "./store/storeRenderer";

// Update the public registry every 5 minutes
const PROVIDER_UPDATE_INTERVAL = 5 * 60 * 1_000;

// Update node statuses every 2 minutes
const STATUS_UPDATE_INTERVAL = 2 * 60 * 1_000;

// Update the exchange rate every 5 minutes
const UPDATE_RATE_INTERVAL = 5 * 60 * 1_000;

function setIntervalImmediate(callback: () => void, interval: number): void {
    callback();
    setInterval(callback, interval);
}

export async function setupBackgroundTasks(): Promise<void> {
    // // Setup periodic fetch tasks
    setIntervalImmediate(updatePublicRegistry, PROVIDER_UPDATE_INTERVAL);
    setIntervalImmediate(updateAllNodeStatuses, STATUS_UPDATE_INTERVAL);
    setIntervalImmediate(updateRates, UPDATE_RATE_INTERVAL);

    // // Setup Tauri event listeners

    // Check if the context is already available. This is to prevent unnecessary re-initialization
    if (await checkContextAvailability()) {
        store.dispatch(contextStatusEventReceived({ type: "Available" }));
    } else {
        // Warning: If we reload the page while the Context is being initialized, this function will throw an error
        initializeContext().catch((e) => {
            logger.error(e, "Failed to initialize context on page load. This might be because we reloaded the page while the context was being initialized");
            // Wait a short time before retrying
            setTimeout(() => {
                initializeContext().catch((e) => {
                    logger.error(e, "Failed to initialize context even after retry");
                });
            }, 2000);
        });
    }

    listen<TauriSwapProgressEventWrapper>("swap-progress-update", (event) => {
        logger.info("Received swap progress event", event.payload);
        store.dispatch(swapProgressEventReceived(event.payload));
    });

    listen<TauriContextStatusEvent>("context-init-progress-update", (event) => {
        logger.info("Received context init progress event", event.payload);
        store.dispatch(contextStatusEventReceived(event.payload));
    });

    listen<TauriLogEvent>("cli-log-emitted", (event) => {
        store.dispatch(receivedCliLog(event.payload));
    });

    listen<BalanceResponse>("balance-change", (event) => {
        logger.info("Received balance change event", event.payload);
        store.dispatch(rpcSetBalance(event.payload.balance));
    });

    listen<TauriDatabaseStateEvent>("swap-database-state-update", (event) => {
        logger.info("Received swap database state update event", event.payload);
        getSwapInfo(event.payload.swap_id);

        // This is ugly but it's the best we can do for now
        // Sometimes we are too quick to fetch the swap info and the new state is not yet reflected
        // in the database. So we wait a bit before fetching the new state
        setTimeout(() => getSwapInfo(event.payload.swap_id), 3000);
    });

    listen<TauriTimelockChangeEvent>('timelock-change', (event) => {
        logger.info('Received timelock change event', event.payload);
        store.dispatch(timelockChangeEventReceived(event.payload));
    })

    listen<TauriBackgroundRefundEvent>('background-refund', (event) => {
        logger.info('Received background refund event', event.payload);
        store.dispatch(rpcSetBackgroundRefundState(event.payload));
    })

    listen<ApprovalRequest>("approval_event", (event) => {
        logger.info("Received approval_event:", event.payload);
        store.dispatch(approvalEventReceived(event.payload));
    });
}