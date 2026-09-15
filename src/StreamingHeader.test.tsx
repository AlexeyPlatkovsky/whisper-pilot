import { fireEvent, render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { StreamingHeader } from "./StreamingHeader";
import { resolveStreamingWidgetStatus } from "./streamingStatus";
import * as ipc from "./ipc";
import type { ComponentProps } from "react";

vi.mock("./ipc", () => ({
  collapseToBubble: vi.fn(async () => undefined),
}));

const props: ComponentProps<typeof StreamingHeader> = {
  activeId: 1,
  activeTitle: "Standup",
  activeBusy: false,
  captureHydrated: true,
  busy: false,
  isRunning: false,
  isStartPending: false,
  isStopPending: false,
  meetingTranscriptionActive: false,
  sidebarOpen: true,
  elapsed: 0,
  widget: resolveStreamingWidgetStatus("ready"),
  windowsCount: 0,
  exportText: "",
  hasText: false,
  craftDisabled: false,
  canClear: false,
  onToggleSidebar: vi.fn(),
  onCreate: vi.fn(),
  onOpenSettings: vi.fn(),
  onRename: vi.fn(),
  onDelete: vi.fn(),
  onStart: vi.fn(),
  onStop: vi.fn(),
  onCraft: vi.fn(),
  onExport: vi.fn(),
  onClear: vi.fn(),
  onError: vi.fn(),
};

describe("StreamingHeader", () => {
  it("collapses to the floating bubble from the logo button", () => {
    const view = render(<StreamingHeader {...props} />);

    fireEvent.click(
      view.getByRole("button", { name: "Collapse to floating bubble" }),
    );

    expect(ipc.collapseToBubble).toHaveBeenCalledOnce();
  });
});
