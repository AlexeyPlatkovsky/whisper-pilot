import {
  useEffect,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  getLiveCaptureSnapshot,
  onLiveCaptureState,
  onRecorderError,
  onRecorderPartial,
  onRecorderSegmentCommitted,
  onRecorderSessionChanged,
  type LiveCaptureSnapshot,
  type RecorderSession,
  type RecorderSessionSummary,
  type RecorderStatus,
} from "./ipc";
import { reconcileLiveCaptureSnapshot } from "./liveCaptureState";
import { stabilizePartial, type StablePartial } from "./partialStability";

type PartialCursor = { sessionId: number | null; revision: number };

/** Registers Recorder events sequentially so a failed later registration can
 * release every earlier native listener without leaving a rejected effect. */
export function useRecorderEventListeners({
  activeId,
  activeStatus,
  openRequest,
  deletedSessionIds,
  previousPartial,
  partialCursor,
  setSnapshot,
  upsertSession,
  displaySession,
  setActive,
  resetPartial,
  setPartialDisplay,
  setPartial,
  setError,
}: {
  activeId: MutableRefObject<number | null>;
  activeStatus: MutableRefObject<RecorderStatus | null>;
  openRequest: MutableRefObject<number>;
  deletedSessionIds: MutableRefObject<Set<number>>;
  previousPartial: MutableRefObject<string>;
  partialCursor: MutableRefObject<PartialCursor>;
  setSnapshot: Dispatch<SetStateAction<LiveCaptureSnapshot | null>>;
  upsertSession: (session: RecorderSessionSummary) => void;
  displaySession: (session: RecorderSession) => void;
  setActive: Dispatch<SetStateAction<RecorderSession | null>>;
  resetPartial: () => void;
  setPartialDisplay: Dispatch<SetStateAction<StablePartial>>;
  setPartial: Dispatch<SetStateAction<string>>;
  setError: Dispatch<SetStateAction<string | null>>;
}) {
  useEffect(() => {
    let cancelled = false;
    const unlisteners: Array<() => void> = [];
    const disposeListeners = () => {
      for (const unlisten of unlisteners.splice(0)) unlisten();
    };
    const subscribe = async (registration: Promise<() => void>) => {
      const unlisten = await registration;
      if (cancelled) {
        unlisten();
        return false;
      }
      unlisteners.push(unlisten);
      return true;
    };
    const register = async () => {
      try {
        if (
          !(await subscribe(
            onLiveCaptureState((next) => {
              if (!cancelled) {
                setSnapshot((current) =>
                  reconcileLiveCaptureSnapshot(current, next),
                );
              }
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onRecorderSessionChanged((session) => {
              if (cancelled || deletedSessionIds.current.has(session.id)) {
                return;
              }
              upsertSession(session);
              if (
                activeId.current === session.id ||
                session.status === "recording"
              ) {
                openRequest.current += 1;
                displaySession(session);
              }
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onRecorderSegmentCommitted((segment) => {
              if (cancelled || activeId.current !== segment.session_id) return;
              setActive((current) => {
                if (!current || current.id !== segment.session_id)
                  return current;
                const withoutSame = current.segments.filter(
                  (item) => item.id !== segment.id,
                );
                return { ...current, segments: [...withoutSame, segment] };
              });
              resetPartial();
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onRecorderPartial((next) => {
              if (
                cancelled ||
                activeId.current !== next.session_id ||
                activeStatus.current !== "recording"
              ) {
                return;
              }
              const cursor = partialCursor.current;
              const previousRevision =
                cursor.sessionId === next.session_id ? cursor.revision : -1;
              if (next.revision <= previousRevision) return;
              partialCursor.current = {
                sessionId: next.session_id,
                revision: next.revision,
              };
              setPartialDisplay(
                stabilizePartial(previousPartial.current, next.text),
              );
              previousPartial.current = next.text;
              setPartial(next.text);
            }),
          ))
        ) {
          return;
        }
        if (
          !(await subscribe(
            onRecorderError((next) => {
              if (
                !cancelled &&
                (next.session_id === undefined ||
                  next.session_id === activeId.current)
              ) {
                setError(next.message);
              }
            }),
          ))
        ) {
          return;
        }
        const initial = await getLiveCaptureSnapshot();
        if (!cancelled) {
          setSnapshot((current) =>
            reconcileLiveCaptureSnapshot(current, initial),
          );
        }
      } catch (reason) {
        disposeListeners();
        if (!cancelled) setError(String(reason));
      }
    };
    void register();
    return () => {
      cancelled = true;
      disposeListeners();
    };
  }, [
    activeId,
    activeStatus,
    displaySession,
    deletedSessionIds,
    openRequest,
    partialCursor,
    previousPartial,
    resetPartial,
    setActive,
    setError,
    setPartial,
    setPartialDisplay,
    setSnapshot,
    upsertSession,
  ]);
}
