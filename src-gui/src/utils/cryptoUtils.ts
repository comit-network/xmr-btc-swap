import { createHash } from "crypto";

export function sha256(data: string): string {
  return createHash("md5").update(data).digest("hex");
}
