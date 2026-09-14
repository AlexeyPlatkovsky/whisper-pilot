import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ConfirmDialog } from "./ConfirmDialog";

describe("ConfirmDialog", () => {
  it("associates the irreversible-action description with the alert dialog", () => {
    render(
      <ConfirmDialog
        label="Clear recording"
        title="Clear recording?"
        description="Audio and transcript will be permanently removed."
        confirmLabel="Clear recording"
        destructive
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
      />,
    );

    expect(screen.getByRole("alertdialog")).toHaveAccessibleDescription(
      "Audio and transcript will be permanently removed.",
    );
  });
});
