import { listen } from "@tauri-apps/api/event";
import { TauriContextStatusEvent, TauriEvent } from "models/tauriModel";
import {
  contextStatusEventReceived,
  receivedCliLog,
  rpcSetBalance,
  timelockChangeEventReceived,
  approvalEventReceived,
  backgroundProgressEventReceived,
} from "store/features/rpcSlice";
import { poolStatusReceived } from "store/features/poolSlice";
import { swapProgressEventReceived } from "store/features/swapSlice";
import logger from "utils/logger";
import {
  fetchAllConversations,
  updateAlerts,
  updatePublicRegistry,
  updateRates,
} from "./api";
import {
  checkContextAvailability,
  getSwapInfo,
  initializeContext,
  listSellersAtRendezvousPoint,
  updateAllNodeStatuses,
} from "./rpc";
import { store } from "./store/storeRenderer";
import { exhaustiveGuard } from "utils/typescriptUtils";

const TAURI_UNIFIED_EVENT_CHANNEL_NAME = "tauri-unified-event";

// Update the public registry every 5 minutes
const PROVIDER_UPDATE_INTERVAL = 5 * 60 * 1_000;

// Discover peers every 5 minutes
const DISCOVER_PEERS_INTERVAL = 5 * 60 * 1_000;

// Update node statuses every 2 minutes
const STATUS_UPDATE_INTERVAL = 2 * 60 * 1_000;

// Update the exchange rate every 5 minutes
const UPDATE_RATE_INTERVAL = 5 * 60 * 1_000;

// Fetch all conversations every 10 minutes
const FETCH_CONVERSATIONS_INTERVAL = 10 * 60 * 1_000;

function setIntervalImmediate(callback: () => void, interval: number): void {
  callback();
  setInterval(callback, interval);
}

export async function setupBackgroundTasks(): Promise<void> {
  // Setup periodic fetch tasks
  setIntervalImmediate(updatePublicRegistry, PROVIDER_UPDATE_INTERVAL);
  setIntervalImmediate(updateAllNodeStatuses, STATUS_UPDATE_INTERVAL);
  setIntervalImmediate(updateRates, UPDATE_RATE_INTERVAL);
  setIntervalImmediate(fetchAllConversations, FETCH_CONVERSATIONS_INTERVAL);
  setIntervalImmediate(
    () =>
      listSellersAtRendezvousPoint(store.getState().settings.rendezvousPoints),
    DISCOVER_PEERS_INTERVAL,
  );

  // Fetch all alerts
  updateAlerts();

  // Setup Tauri event listeners
  // Check if the context is already available. This is to prevent unnecessary re-initialization
  if (await checkContextAvailability()) {
    store.dispatch(
      contextStatusEventReceived(TauriContextStatusEvent.Available),
    );
  } else {
    // Warning: If we reload the page while the Context is being initialized, this function will throw an error
    initializeContext().catch((e) => {
      logger.error(
        e,
        "Failed to initialize context on page load. This might be because we reloaded the page while the context was being initialized",
      );
      // Wait a short time before retrying
      setTimeout(() => {
        initializeContext().catch((e) => {
          logger.error(e, "Failed to initialize context even after retry");
        });
      }, 2000);
    });
  }

  // Listen for the unified event
  listen<TauriEvent>(TAURI_UNIFIED_EVENT_CHANNEL_NAME, (event) => {
    const { channelName, event: eventData } = event.payload;

    switch (channelName) {
      case "SwapProgress":
        store.dispatch(swapProgressEventReceived(eventData));
        break;

      case "ContextInitProgress":
        store.dispatch(contextStatusEventReceived(eventData));
        break;

      case "CliLog":
        store.dispatch(receivedCliLog(eventData));
        break;

      case "BalanceChange":
        store.dispatch(rpcSetBalance(eventData.balance));
        break;

      case "SwapDatabaseStateUpdate":
        getSwapInfo(eventData.swap_id);

        // This is ugly but it's the best we can do for now
        // Sometimes we are too quick to fetch the swap info and the new state is not yet reflected
        // in the database. So we wait a bit before fetching the new state
        setTimeout(() => getSwapInfo(eventData.swap_id), 3000);
        break;

      case "TimelockChange":
        store.dispatch(timelockChangeEventReceived(eventData));
        break;

      case "Approval":
        store.dispatch(approvalEventReceived(eventData));
        break;

      case "BackgroundProgress":
        store.dispatch(backgroundProgressEventReceived(eventData));
        break;

      case "PoolStatusUpdate":
        store.dispatch(poolStatusReceived(eventData));
        break;

      default:
        exhaustiveGuard(channelName);
    }
  });
}
