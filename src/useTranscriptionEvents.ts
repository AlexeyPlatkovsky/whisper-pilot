import {
  useEffect,
  useRef,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  onTranscriptionPhase,
  onTranscriptionProgress,
  openMeeting,
  type Meeting,
  type MeetingMfu,
  type Segment,
} from "./ipc";

type TranscriptionEventsOptions = {
  transcribingId: number | null;
  activeMeetingIdRef: MutableRefObject<number | null>;
  setTranscribingPhase: Dispatch<SetStateAction<"transcribing" | "diarizing">>;
  setTranscribingProgress: Dispatch<SetStateAction<number | null>>;
  setActiveMeeting: Dispatch<SetStateAction<Meeting | null>>;
  setFileName: Dispatch<SetStateAction<string | null>>;
  setSegments: Dispatch<SetStateAction<Segment[]>>;
  setSpeakerLabels: Dispatch<SetStateAction<Record<number, string>>>;
  setMfu: Dispatch<SetStateAction<MeetingMfu | null>>;
  upsertSummary: (meeting: Meeting) => void;
};

/** Reconciles native transcription progress and phase events with the active workspace. */
export function useTranscriptionEvents({
  transcribingId,
  activeMeetingIdRef,
  setTranscribingPhase,
  setTranscribingProgress,
  setActiveMeeting,
  setFileName,
  setSegments,
  setSpeakerLabels,
  setMfu,
  upsertSummary,
}: TranscriptionEventsOptions) {
  const transcribingIdRef = useRef<number | null>(null);

  useEffect(() => {
    transcribingIdRef.current = transcribingId;
  }, [transcribingId]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    onTranscriptionPhase((event) => {
      if (event.id !== transcribingIdRef.current) return;

      setTranscribingPhase(event.phase);
      setTranscribingProgress(null);

      // Segments are durable before diarization begins, so make that completed
      // work readable immediately without pulling the user to another item.
      if (
        event.phase === "diarizing" &&
        activeMeetingIdRef.current === event.id
      ) {
        void Promise.resolve()
          .then(() => openMeeting(event.id))
          .then((meeting) => {
            if (!meeting || activeMeetingIdRef.current !== meeting.id) return;
            setActiveMeeting(meeting);
            setFileName(meeting.source_name ?? null);
            setSegments(meeting.segments);
            setSpeakerLabels({});
            setMfu(meeting.mfu ?? null);
            upsertSummary(meeting);
          })
          .catch(() => {});
      }
    }).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => unlisten?.();
  }, [
    activeMeetingIdRef,
    setActiveMeeting,
    setFileName,
    setMfu,
    setSegments,
    setSpeakerLabels,
    setTranscribingPhase,
    setTranscribingProgress,
    upsertSummary,
  ]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    onTranscriptionProgress((event) => {
      if (event.id !== transcribingIdRef.current) return;
      setTranscribingProgress(Math.max(0, Math.min(100, event.percent)));
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [setTranscribingProgress]);
}
