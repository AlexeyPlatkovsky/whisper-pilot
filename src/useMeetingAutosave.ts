import {
  useEffect,
  useRef,
  type Dispatch,
  type RefObject,
  type SetStateAction,
} from "react";
import { updateMfu, updateSegment, type MeetingMfu, type Segment } from "./ipc";

const AUTOSAVE_DEBOUNCE_MS = 500;

type PendingSegment = {
  timer: ReturnType<typeof setTimeout>;
  meetingId: number;
  index: number;
  text: string;
};

type PendingMfu = { timer: ReturnType<typeof setTimeout>; mfu: MeetingMfu };

/**
 * Owns debounced editor persistence. The app shell owns meeting navigation;
 * writes retain their meeting id so navigation cannot reroute an old edit.
 */
export function useMeetingAutosave({
  activeMeetingId,
  activeMeetingIdRef,
  mfu,
  setMfu,
  setSegments,
  onError,
}: {
  activeMeetingId: number | undefined;
  activeMeetingIdRef: RefObject<number | null>;
  mfu: MeetingMfu | null;
  setMfu: Dispatch<SetStateAction<MeetingMfu | null>>;
  setSegments: Dispatch<SetStateAction<Segment[]>>;
  onError: (message: string) => void;
}) {
  const segmentSaveTimers = useRef<Map<string, PendingSegment>>(new Map());
  const mfuSaveTimers = useRef<Map<number, PendingMfu>>(new Map());
  const autosaveWrites = useRef<
    Set<{ meetingId: number; write: Promise<unknown> }>
  >(new Set());
  const mfuRef = useRef<MeetingMfu | null>(mfu);

  useEffect(() => {
    mfuRef.current = mfu;
  }, [mfu]);

  function track<T>(meetingId: number, write: Promise<T>): Promise<T> {
    const pending = { meetingId, write };
    autosaveWrites.current.add(pending);
    void write.then(
      () => autosaveWrites.current.delete(pending),
      (error) => {
        autosaveWrites.current.delete(pending);
        if (activeMeetingIdRef.current === meetingId) onError(String(error));
      },
    );
    return write;
  }

  function editSegment(index: number, text: string) {
    setSegments((previous) =>
      previous.map((segment, current) =>
        current === index ? { ...segment, text } : segment,
      ),
    );
    if (activeMeetingId === undefined) return;
    const key = `${activeMeetingId}:${index}`;
    const pending = segmentSaveTimers.current.get(key);
    if (pending) clearTimeout(pending.timer);
    segmentSaveTimers.current.set(key, {
      meetingId: activeMeetingId,
      index,
      text,
      timer: setTimeout(() => {
        segmentSaveTimers.current.delete(key);
        track(activeMeetingId, updateSegment(activeMeetingId, index, text));
      }, AUTOSAVE_DEBOUNCE_MS),
    });
  }

  function editMfuField(field: keyof MeetingMfu, value: string) {
    if (field === "meeting_id" || activeMeetingId === undefined) return;
    const current = mfuRef.current;
    if (!current) return;
    const next = { ...current, meeting_id: activeMeetingId, [field]: value };
    mfuRef.current = next;
    setMfu(next);
    const pending = mfuSaveTimers.current.get(activeMeetingId);
    if (pending) clearTimeout(pending.timer);
    mfuSaveTimers.current.set(activeMeetingId, {
      mfu: next,
      timer: setTimeout(() => {
        mfuSaveTimers.current.delete(activeMeetingId);
        track(activeMeetingId, updateMfu(next));
      }, AUTOSAVE_DEBOUNCE_MS),
    });
  }

  async function flushMeetingWrites(meetingId: number) {
    const writes = [...autosaveWrites.current]
      .filter((pending) => pending.meetingId === meetingId)
      .map((pending) => pending.write);
    for (const [key, pending] of segmentSaveTimers.current) {
      if (pending.meetingId !== meetingId) continue;
      clearTimeout(pending.timer);
      segmentSaveTimers.current.delete(key);
      writes.push(
        track(meetingId, updateSegment(meetingId, pending.index, pending.text)),
      );
    }
    const pendingMfu = mfuSaveTimers.current.get(meetingId);
    if (pendingMfu) {
      clearTimeout(pendingMfu.timer);
      mfuSaveTimers.current.delete(meetingId);
      writes.push(track(meetingId, updateMfu(pendingMfu.mfu)));
    }
    await Promise.allSettled([...new Set(writes)]);
  }

  return { editSegment, editMfuField, flushMeetingWrites };
}
