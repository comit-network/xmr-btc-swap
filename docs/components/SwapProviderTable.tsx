import { useState, useEffect } from "react";
import { Table, Td, Th, Tr } from 'nextra/components'

export default function SwapMakerTable() {
  function satsToBtc(sats) {
    return sats / 100000000;
  }

  async function getMakers() {
    const response = await fetch("https://api.unstoppableswap.net/api/list");
    const data = await response.json();
    return data;
  }

  const [makers, setMakers] = useState([]);

  useEffect(() => {
    getMakers().then((data) => {
      setMakers(data);
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
          {makers.map((maker) => (
            <Tr key={maker.peerId}>
              <Td>
                {maker.testnet ? "Testnet" : "Mainnet"}
              </Td>
              <Td>{maker.multiAddr}</Td>
              <Td>{maker.peerId}</Td>
              <Td>{satsToBtc(maker.minSwapAmount)} BTC</Td>
              <Td>{satsToBtc(maker.maxSwapAmount)} BTC</Td>
              <Td>{satsToBtc(maker.price)} XMR/BTC</Td>
            </Tr>
          ))}
        </tbody>
      </Table>
    </div>
  );
}
