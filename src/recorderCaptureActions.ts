import {
  createRecorderDraft,
  getMicrophonePermissionStatus,
  requestMicrophonePermission,
  startRecorderSession,
  type RecorderSession,
} from "./ipc";

export async function createRecorderDraftAction({
  onCreated,
  onError,
}: {
  onCreated: (session: RecorderSession) => void;
  onError: (error: unknown) => void;
}) {
  try {
    onCreated(await createRecorderDraft());
  } catch (error) {
    onError(error);
  }
}

export async function startRecorderCaptureAction({
  sessionId,
  onStarted,
  onError,
}: {
  sessionId: number | undefined;
  onStarted: (session: RecorderSession) => void;
  onError: (error: unknown) => void;
}) {
  try {
    let permission = await getMicrophonePermissionStatus();
    if (permission === "not_determined") {
      permission = await requestMicrophonePermission();
    }
    if (permission !== "authorized") {
      onError(`Microphone permission is ${permission.replace("_", " ")}.`);
      return;
    }
    onStarted(await startRecorderSession(sessionId));
  } catch (error) {
    onError(error);
  }
}

export async function runRecorderAction(
  action: () => Promise<unknown>,
  onError: (error: string) => void,
) {
  try {
    await action();
  } catch (error) {
    onError(String(error));
  }
}
