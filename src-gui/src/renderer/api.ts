// This file is responsible for making HTTP requests to the Unstoppable API and to the CoinGecko API.
// The APIs are used to:
// - fetch provider status from the public registry
// - fetch alerts to be displayed to the user
// - and to submit feedback
// - fetch currency rates from CoinGecko
import { Alert, ExtendedProviderStatus } from "models/apiModel";

const API_BASE_URL = "https://api.unstoppableswap.net";

export async function fetchProvidersViaHttp(): Promise<
  ExtendedProviderStatus[]
> {
  const response = await fetch(`${API_BASE_URL}/api/list`);
  return (await response.json()) as ExtendedProviderStatus[];
}

export async function fetchAlertsViaHttp(): Promise<Alert[]> {
  const response = await fetch(`${API_BASE_URL}/api/alerts`);
  return (await response.json()) as Alert[];
}

export async function submitFeedbackViaHttp(
  body: string,
  attachedData: string,
): Promise<string> {
  type Response = {
    feedbackId: string;
  };

  const response = await fetch(`${API_BASE_URL}/api/submit-feedback`, {
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

async function fetchCurrencyUsdPrice(currency: string): Promise<number> {
  try {
    const response = await fetch(
      `https://api.coingecko.com/api/v3/simple/price?ids=${currency}&vs_currencies=usd`,
    );
    const data = await response.json();
    return data[currency].usd;
  } catch (error) {
    console.error(`Error fetching ${currency} price:`, error);
    throw error;
  }
}

export async function fetchXmrBtcRate(): Promise<number> {
  try {
    const response = await fetch('https://api.kraken.com/0/public/Ticker?pair=XMRXBT');
    const data = await response.json();
    
    if (data.error && data.error.length > 0) {
      throw new Error(`Kraken API error: ${data.error[0]}`);
    }

    const result = data.result.XXMRXXBT;
    const lastTradePrice = parseFloat(result.c[0]);

    return lastTradePrice;
  } catch (error) {
    console.error('Error fetching XMR/BTC rate from Kraken:', error);
    throw error;
  }
}


export async function fetchBtcPrice(): Promise<number> {
  return fetchCurrencyUsdPrice("bitcoin");
}

export async function fetchXmrPrice(): Promise<number> {
  return fetchCurrencyUsdPrice("monero");
}