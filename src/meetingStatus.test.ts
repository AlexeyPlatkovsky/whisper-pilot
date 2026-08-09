import { describe, expect, it } from "vitest";
import { resolveMeetingStatus } from "./meetingStatus";

describe("resolveMeetingStatus — persisted statuses", () => {
  // EP: the valid partition — every status src-tauri/src/meetings.rs can write.
  it("maps every persisted status the store can produce to one label and tone", () => {
    expect(resolveMeetingStatus("no_files")).toEqual({
      tone: "no-files",
      label: "No files",
      icon: "info",
    });
    expect(resolveMeetingStatus("ready")).toEqual({
      tone: "ready",
      label: "Ready",
      icon: "check",
    });
    expect(resolveMeetingStatus("finished")).toEqual({
      tone: "finished",
      label: "Finished",
      icon: "check",
    });
  });
});

describe("resolveMeetingStatus — transient activity overrides", () => {
  it("reports transcribing regardless of the persisted status", () => {
    // A meeting keeps its stored status ("ready") for the whole run, so the
    // in-flight activity — not the store — decides what the UI shows.
    expect(resolveMeetingStatus("ready", "transcribing")).toEqual({
      tone: "transcribing",
      label: "Transcribing",
      icon: "refresh-cw",
    });
    expect(resolveMeetingStatus("finished", "transcribing")).toEqual({
      tone: "transcribing",
      label: "Transcribing",
      icon: "refresh-cw",
    });
  });

  it("reports diarizing regardless of the persisted status", () => {
    // Diarization runs after transcription completes, on the same in-flight
    // meeting, so it gets its own tone/label distinct from "transcribing".
    expect(resolveMeetingStatus("ready", "diarizing")).toEqual({
      tone: "diarizing",
      label: "Diarizing",
      icon: "refresh-cw",
    });
    expect(resolveMeetingStatus("finished", "diarizing")).toEqual({
      tone: "diarizing",
      label: "Diarizing",
      icon: "refresh-cw",
    });
  });

  it("reports the no-model tone when a transcription model is not available", () => {
    expect(resolveMeetingStatus("ready", "no-model")).toEqual({
      tone: "no-model",
      label: "No model",
      icon: "alert-circle",
    });
  });

  it("reports the error tone when the last action on the meeting failed", () => {
    expect(resolveMeetingStatus("ready", "error")).toEqual({
      tone: "error",
      label: "Error",
      icon: "alert-circle",
    });
  });

  it("defaults to no activity so a plain status call is unambiguous", () => {
    expect(resolveMeetingStatus("finished", "none")).toEqual(
      resolveMeetingStatus("finished"),
    );
  });
});

describe("resolveMeetingStatus — invalid and missing input", () => {
  // EP: invalid partition — a value outside the store's vocabulary.
  it("does not invent a status for an unrecognised store value", () => {
    expect(resolveMeetingStatus("transmogrifying")).toEqual({
      tone: "unknown",
      label: "Unknown",
      icon: "alert-circle",
    });
  });

  // EP: absent/empty partition.
  it("does not invent a status when the meeting has none", () => {
    expect(resolveMeetingStatus(undefined)).toEqual({
      tone: "unknown",
      label: "Unknown",
      icon: "alert-circle",
    });
    expect(resolveMeetingStatus("")).toEqual({
      tone: "unknown",
      label: "Unknown",
      icon: "alert-circle",
    });
  });

  // EP: wrong-kind partition — a value that collides with an Object.prototype
  // key must not resolve through the prototype chain.
  it("does not resolve a prototype key as if it were a status", () => {
    expect(resolveMeetingStatus("constructor")).toEqual({
      tone: "unknown",
      label: "Unknown",
      icon: "alert-circle",
    });
    expect(resolveMeetingStatus("toString")).toEqual({
      tone: "unknown",
      label: "Unknown",
      icon: "alert-circle",
    });
  });

  it("still honours a transient override on an unrecognised value", () => {
    expect(resolveMeetingStatus("transmogrifying", "transcribing")).toEqual({
      tone: "transcribing",
      label: "Transcribing",
      icon: "refresh-cw",
    });
  });
});
