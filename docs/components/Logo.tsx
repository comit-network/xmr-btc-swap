import Image from "next/image";

export default function Logo() {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
      <Image
        src="/favicon.svg"
        alt="eigenwallet"
        width={32}
        height={32}
        style={{ borderRadius: "20%" }}
      />
      <span>eigenwallet</span>
    </div>
  );
}
