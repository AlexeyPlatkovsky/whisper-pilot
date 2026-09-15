import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { RecorderDialogs } from "./RecorderDialogs";

describe("RecorderDialogs", () => {
  it("cancels an active rename dialog with Escape", () => {
    const onRenameCancel = vi.fn();

    render(
      <RecorderDialogs
        renameTarget={{ id: 1, title: "Draft" }}
        renameDraft="Draft"
        renameError={null}
        onRenameDraftChange={vi.fn()}
        onRenameCancel={onRenameCancel}
        onSaveRename={vi.fn()}
        deleteTarget={null}
        onDeleteCancel={vi.fn()}
        onDelete={vi.fn()}
        clearPending={false}
        active={null}
        onClearCancel={vi.fn()}
        onClear={vi.fn()}
      />,
    );

    fireEvent.keyDown(
      screen.getByRole("dialog", { name: "Rename recording" }),
      {
        key: "Escape",
      },
    );

    expect(onRenameCancel).toHaveBeenCalledOnce();
  });
});
