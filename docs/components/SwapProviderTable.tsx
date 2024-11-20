import { useState, useEffect } from "react";
import { Table, Td, Th, Tr } from 'nextra/components'

export default function SwapProviderTable() {
  function satsToBtc(sats) {
    return sats / 100000000;
  }

  async function getProviders() {
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
      <Table>
        <thead>
          <Tr>
            <Th>Network</Th>
            <Th>Multiaddress</Th>
            <Th>Peer ID</Th>
            <Th>Minimum Amount</Th>
            <Th>Maximum Amount</Th>
            <Th>Exchange Rate</Th>
          </Tr>
        </thead>
        <tbody>
          {providers.map((provider) => (
            <Tr key={provider.peerId}>
              <Td>
                {provider.testnet ? "Testnet" : "Mainnet"}
              </Td>
              <Td>{provider.multiAddr}</Td>
              <Td>{provider.peerId}</Td>
              <Td>{satsToBtc(provider.minSwapAmount)} BTC</Td>
              <Td>{satsToBtc(provider.maxSwapAmount)} BTC</Td>
              <Td>{satsToBtc(provider.price)} XMR/BTC</Td>
            </Tr>
          ))}
        </tbody>
      </Table>
    </div>
  );
}
