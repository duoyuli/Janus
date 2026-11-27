// The colours a status is drawn in, and which of the service's run states are at work (the Mac
// app's `RunStatus.isActive`). What a run is called and how its state reads come from the service
// (`title` and `label` of every listed run).

export type Tone = "gray" | "blue" | "green" | "amber" | "red" | "violet";

/** A run in one of these states is at work: queued, its lead starting, or running. */
export const ACTIVE = new Set(["queued", "planning", "running"]);
