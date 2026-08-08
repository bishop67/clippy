import { invokeCommand } from "../lib/tauri";
import { ClipboardModel } from "../types";
import { ClipboardType } from "../types/enums";
import { InvokeCommand } from "../types/tauri-invoke";

// Hiding the window and pasting into the previously focused one happen
// backend-side, so every way of picking an entry behaves the same.
export const copyEntry = (id: ClipboardModel["id"], type: ClipboardType) =>
  invokeCommand(InvokeCommand.CopyClipboard, { id, type });
