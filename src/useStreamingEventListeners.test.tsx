import { act, render, waitFor } from "@testing-library/react";
import { useRef, useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { useStreamingEventListeners } from "./useStreamingEventListeners";
import type { StreamingWindow } from "./ipc";

type Handler<T> = (payload: T) => void;

const registrations = vi.hoisted(() => ({
  window: vi.fn(),
  sources: vi.fn(),
  ended: vi.fn(),
  partial: vi.fn(),
  error: vi.fn(),
}));

let windowHandler: Handler<StreamingWindow & { session_id: number }> | null =
  null;
let sourcesHandler: Handler<{ mic: boolean; system_audio: boolean }> | null =
  null;
let endedHandler: Handler<{ session_id: number }> | null = null;
let partialHandler: Handler<{
  session_id: number;
  item_id: string | null;
  text: string;
}> | null = null;
let errorHandler: Handler<{ session_id: number; message: string }> | null =
  null;

vi.mock("./ipc", () => ({
  onStreamingWindow: registrations.window,
  onStreamingSources: registrations.sources,
  onStreamingSessionEnded: registrations.ended,
  onStreamingPartial: registrations.partial,
  onStreamingError: registrations.error,
}));

function Harness({
  refreshSessions,
}: {
  refreshSessions: () => Promise<void>;
}) {
  const activeIdRef = useRef<number | null>(1);
  const partialTranscriptRef = useRef<{
    itemId: string | null;
    text: string;
  } | null>({
    itemId: "draft",
    text: "old draft",
  });
  const previewGenerationRef = useRef(0);
  const [, setActiveId] = useState<number | null>(1);
  const [, setWindows] = useState<StreamingWindow[]>([]);
  const [, setPartialTranscript] = useState<{
    itemId: string | null;
    text: string;
  } | null>(null);
  const [, setSources] = useState<{
    mic: boolean;
    system_audio: boolean;
  } | null>(null);
  const [, setError] = useState<string | null>(null);

  useStreamingEventListeners({
    activeIdRef,
    partialTranscriptRef,
    previewGenerationRef,
    setActiveId,
    setWindows,
    setPartialTranscript,
    setSources,
    setError,
    refreshSessions,
  });
  return null;
}

function resetRegistrations() {
  vi.clearAllMocks();
  windowHandler = null;
  sourcesHandler = null;
  endedHandler = null;
  partialHandler = null;
  errorHandler = null;
  registrations.window.mockImplementation(
    async (handler: typeof windowHandler) => {
      windowHandler = handler;
      return () => {};
    },
  );
  registrations.sources.mockImplementation(
    async (handler: typeof sourcesHandler) => {
      sourcesHandler = handler;
      return () => {};
    },
  );
  registrations.ended.mockImplementation(
    async (handler: typeof endedHandler) => {
      endedHandler = handler;
      return () => {};
    },
  );
  registrations.partial.mockImplementation(
    async (handler: typeof partialHandler) => {
      partialHandler = handler;
      return () => {};
    },
  );
  registrations.error.mockImplementation(
    async (handler: typeof errorHandler) => {
      errorHandler = handler;
      return () => {};
    },
  );
}

describe("useStreamingEventListeners", () => {
  it("routes matching events and safely contains a refresh failure", async () => {
    resetRegistrations();
    const refreshSessions = vi.fn().mockRejectedValue(new Error("store busy"));
    render(<Harness refreshSessions={refreshSessions} />);

    await waitFor(() => {
      expect(windowHandler).not.toBeNull();
      expect(sourcesHandler).not.toBeNull();
      expect(endedHandler).not.toBeNull();
      expect(partialHandler).not.toBeNull();
      expect(errorHandler).not.toBeNull();
    });

    act(() => {
      windowHandler?.({
        session_id: 1,
        window_index: 0,
        start_ms: 0,
        end_ms: 1000,
        language: "en",
        text: "committed",
        outcome_ok: true,
        item_id: "draft",
      });
      sourcesHandler?.({ mic: true, system_audio: false });
      partialHandler?.({ session_id: 1, item_id: "next", text: "partial" });
      errorHandler?.({ session_id: 1, message: "recoverable" });
      endedHandler?.({ session_id: 1 });
    });

    expect(refreshSessions).toHaveBeenCalledOnce();
    await Promise.resolve();
  });

  it("stops at a deferred session-ended registration after unmount", async () => {
    resetRegistrations();
    let resolveRegistration!: (unlisten: () => void) => void;
    const registration = new Promise<() => void>((resolve) => {
      resolveRegistration = resolve;
    });
    const unlisten = vi.fn();
    registrations.ended.mockImplementationOnce(
      async (handler: typeof endedHandler) => {
        endedHandler = handler;
        return registration;
      },
    );

    const { unmount } = render(<Harness refreshSessions={vi.fn()} />);
    await waitFor(() => expect(registrations.ended).toHaveBeenCalledOnce());
    unmount();
    await act(async () => resolveRegistration(unlisten));

    expect(unlisten).toHaveBeenCalledOnce();
    expect(registrations.partial).not.toHaveBeenCalled();
  });

  it("disposes listeners when registration throws synchronously", async () => {
    resetRegistrations();
    registrations.window.mockImplementationOnce(() => {
      throw new Error("native listener unavailable");
    });
    render(<Harness refreshSessions={vi.fn()} />);

    await waitFor(() => expect(registrations.window).toHaveBeenCalledOnce());
    expect(registrations.sources).not.toHaveBeenCalled();
  });
});
