export interface ExtendedMakerStatus extends MakerStatus {
  uptime?: number;
  age?: number;
  relevancy?: number;
  version?: string;
  recommended?: boolean;
}

export interface MakerStatus extends MakerQuote, Maker { }

export interface MakerQuote {
  price: number;
  minSwapAmount: number;
  maxSwapAmount: number;
}

export interface Maker {
  multiAddr: string;
  testnet: boolean;
  peerId: string;
}

export interface Alert {
  id: number;
  title: string;
  body: string;
  severity: "info" | "warning" | "error";
}

// Define the correct 9-element tuple type for PrimitiveDateTime
export type PrimitiveDateTimeString = [
    number, // Year
    number, // Day of Year
    number, // Hour
    number, // Minute
    number, // Second
    number, // Nanosecond
    number, // Offset Hour
    number, // Offset Minute
    number  // Offset Second
]; 

export interface Feedback {
  id: string;
  created_at: PrimitiveDateTimeString;
}

export interface Attachment {
  id: number; 
  message_id: number;
  key: string;
  content: string;
  created_at: PrimitiveDateTimeString;
}

export interface Message {
  id: number;
  feedback_id: string;
  is_from_staff: boolean;
  content: string;
  created_at: PrimitiveDateTimeString;
  attachments?: Attachment[];
}

export interface MessageWithAttachments {
  message: Message;
  attachments: Attachment[];
}

// Define type for Attachment data in request body
export interface AttachmentInput {
  key: string;
  content: string;
}
