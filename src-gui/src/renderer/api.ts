// This file is responsible for making HTTP requests to the Unstoppable API and to the CoinGecko API.
// The APIs are used to:
// - fetch provider status from the public registry
// - fetch alerts to be displayed to the user
// - and to submit feedback
// - fetch currency rates from CoinGecko

import { Alert, ExtendedMakerStatus } from "models/apiModel";
import { store } from "./store/storeRenderer";
import { setBtcPrice, setXmrBtcRate, setXmrPrice } from "store/features/ratesSlice";
import { FiatCurrency } from "store/features/settingsSlice";
import { setAlerts } from "store/features/alertsSlice";
import { registryConnectionFailed, setRegistryMakers } from "store/features/makersSlice";
import logger from "utils/logger";

const PUBLIC_REGISTRY_API_BASE_URL = "https://api.unstoppableswap.net";

async function fetchMakersViaHttp(): Promise<
  ExtendedMakerStatus[]
> {
  const response = await fetch(`${PUBLIC_REGISTRY_API_BASE_URL}/api/list`);
  return (await response.json()) as ExtendedMakerStatus[];
}

async function fetchAlertsViaHttp(): Promise<Alert[]> {
  const response = await fetch(`${PUBLIC_REGISTRY_API_BASE_URL}/api/alerts`);
  return (await response.json()) as Alert[];
}

export async function submitFeedbackViaHttp(
  body: string,
  attachedData: string,
): Promise<string> {
  type Response = {
    feedbackId: string;
  };

  const response = await fetch(`${PUBLIC_REGISTRY_API_BASE_URL}/api/submit-feedback`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ body, attachedData }),
  });

  if (!response.ok) {
    throw new Error(`Status: ${response.status}`);
  }

  const responseBody = (await response.json()) as Response;

  return responseBody.feedbackId;
}

async function fetchCurrencyPrice(currency: string, fiatCurrency: FiatCurrency): Promise<number> {
  const response = await fetch(
    `https://api.coingecko.com/api/v3/simple/price?ids=${currency}&vs_currencies=${fiatCurrency.toLowerCase()}`,
  );
  const data = await response.json();
  return data[currency][fiatCurrency.toLowerCase()];
}

async function fetchXmrBtcRate(): Promise<number> {
  const response = await fetch('https://api.kraken.com/0/public/Ticker?pair=XMRXBT');
  const data = await response.json();

  if (data.error && data.error.length > 0) {
    throw new Error(`Kraken API error: ${data.error[0]}`);
  }

  const result = data.result.XXMRXXBT;
  const lastTradePrice = parseFloat(result.c[0]);

  return lastTradePrice;
}


function fetchBtcPrice(fiatCurrency: FiatCurrency): Promise<number> {
  return fetchCurrencyPrice("bitcoin", fiatCurrency);
}

async function fetchXmrPrice(fiatCurrency: FiatCurrency): Promise<number> {
  return fetchCurrencyPrice("monero", fiatCurrency);
}

/**
 * If enabled by the user, fetch the XMR, BTC and XMR/BTC rates 
 * and store them in the Redux store.
 */
export async function updateRates(): Promise<void> {
  const settings = store.getState().settings;
  if (!settings.fetchFiatPrices)
    return;

  try {
    const xmrBtcRate = await fetchXmrBtcRate();
    store.dispatch(setXmrBtcRate(xmrBtcRate));

    const btcPrice = await fetchBtcPrice(settings.fiatCurrency);
    store.dispatch(setBtcPrice(btcPrice));

    const xmrPrice = await fetchXmrPrice(settings.fiatCurrency);
    store.dispatch(setXmrPrice(xmrPrice));

    logger.info(`Fetched rates for ${settings.fiatCurrency}`);
  } catch (error) {
    logger.error(error, "Error fetching rates");
  }
}


/**
 * Update public registry
 */
export async function updatePublicRegistry(): Promise<void> {
  try {
    const providers = await fetchMakersViaHttp();
    store.dispatch(setRegistryMakers(providers));
  } catch (error) {
    store.dispatch(registryConnectionFailed());
    logger.error(error, "Error fetching providers");
  }

  try {
    const alerts = await fetchAlertsViaHttp();
    store.dispatch(setAlerts(alerts));
  } catch (error) {
    logger.error(error, "Error fetching alerts");
  }
}