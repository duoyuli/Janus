// How a run's status reads in the lists: the word and color the Mac app's `RunStatus.listLabel`
// (Core/Status.swift) gives each of the service's states. A mapping of names, not a judgment: the
// state itself is the service's.

import { t } from "./i18n.ts";
import type { Json } from "./api.ts";

export type Tone = "gray" | "blue" | "green" | "amber" | "red" | "violet";

export const ACTIVE = new Set(["queued", "planning", "running"]);

export function listLabel(status: string): [string, Tone] {
  switch (status) {
    case "queued":
      return [t`排队中`, "gray"];
    case "planning":
      return [t`负责人上岗中`, "blue"];
    case "running":
      return [t`干活中`, "blue"];
    case "delivered":
      return [t`已交付`, "green"];
    case "accepted":
      return [t`已验收`, "green"];
    case "failed":
      return [t`失败`, "red"];
    case "cancelled":
      return [t`已停止`, "gray"];
    case "interrupted":
      return [t`已中断`, "gray"];
    default:
      return [t`未知`, "gray"];
  }
}

/** A run row's state: a queued one also says where it is in the queue. */
export function runLabel(r: Json): [string, Tone] {
  if (r.status === "queued" && typeof r.queuePosition === "number") return [t`排队中 · 第 ${r.queuePosition} 位`, "gray"];
  return listLabel(r.status);
}
