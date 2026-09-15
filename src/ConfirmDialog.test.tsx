import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

  it("[WP-130] runs a destructive confirmation only once while its action is in flight", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn();
    render(
      <ConfirmDialog
        label="Delete transcription"
        title="Delete transcription?"
        description="This cannot be undone."
        confirmLabel="Delete"
        destructive
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />,
    );

    const confirm = screen.getByRole("button", { name: "Delete" });
    await user.click(confirm);
    await user.keyboard("{Enter}{Space}");

    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("restores confirmation controls after a synchronous action failure", async () => {
    const user = userEvent.setup();
    const onConfirm = vi.fn<() => void>().mockImplementationOnce(() => {
      throw new Error("delete unavailable");
    });
    render(
      <ConfirmDialog
        label="Delete transcription"
        title="Delete transcription?"
        description="This cannot be undone."
        confirmLabel="Delete"
        destructive
        onCancel={vi.fn()}
        onConfirm={onConfirm}
      />,
    );

    const confirm = screen.getByRole("button", { name: "Delete" });
    await user.click(confirm);
    await waitFor(() => expect(confirm).toBeEnabled());
  });
});
