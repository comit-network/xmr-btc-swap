import { useState, useEffect } from "react";

export default function SwapProviderTable() {
  function satsToBtc(sats) {
    return sats / 100000000;
  }

  async function getProviders() {
    // from https://unstoppableswap.net/api/list with cors disabled
    const response = await fetch("https://api.unstoppableswap.net/api/list");
    const data = await response.json();
    return data;
  }

  const [providers, setProviders] = useState([]);

  useEffect(() => {
    getProviders().then((data) => {
      setProviders(data);
    });
  }, []);

  return (
    <div
      style={{
        overflowX: "scroll",
      }}
    >
      <table>
        <thead>
          <tr>
            <th>Multiaddress</th>
            <th>Peer ID</th>
            <th>Minimum Amount</th>
            <th>Maximum Amount</th>
            <th>Exchange Rate</th>
            <th>Uptime</th>
          </tr>
        </thead>
        <tbody>
          {providers.map((provider) => (
            <tr key={provider.peerId}>
              <td>{provider.multiAddr}</td>
              <td>{provider.peerId}</td>
              <td>{satsToBtc(provider.minSwapAmount)} BTC</td>
              <td>{satsToBtc(provider.maxSwapAmount)} BTC</td>
              <td>{satsToBtc(provider.price)} XMR/BTC</td>
              <td>{(provider.uptime * 100).toFixed(1)}%</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
