import { useCallback, useRef, type Dispatch, type SetStateAction } from "react";
import {
  openMeeting,
  type Meeting,
  type MeetingMfu,
  type MeetingSummary,
  type Segment,
} from "./ipc";
import { toSummary } from "./meetingUtils";

type Status = { kind: "idle" } | { kind: "error"; message: string };

type MeetingSelectionOptions = {
  setActiveMeeting: Dispatch<SetStateAction<Meeting | null>>;
  setFileName: Dispatch<SetStateAction<string | null>>;
  setSegments: Dispatch<SetStateAction<Segment[]>>;
  setEditingSegmentIndex: Dispatch<SetStateAction<number | null>>;
  setSpeakerLabels: Dispatch<SetStateAction<Record<number, string>>>;
  setStatus: Dispatch<SetStateAction<Status>>;
  setMfu: Dispatch<SetStateAction<MeetingMfu | null>>;
  setMeetingSummaries: Dispatch<SetStateAction<MeetingSummary[]>>;
};

/** Keeps one current selection across every asynchronous meeting open path. */
export function useMeetingSelection({
  setActiveMeeting,
  setFileName,
  setSegments,
  setEditingSegmentIndex,
  setSpeakerLabels,
  setStatus,
  setMfu,
  setMeetingSummaries,
}: MeetingSelectionOptions) {
  const activeMeetingIdRef = useRef<number | null>(null);
  const meetingOpenGenerationRef = useRef(0);

  const applyActiveMeeting = useCallback(
    (meeting: Meeting) => {
      activeMeetingIdRef.current = meeting.id;
      setActiveMeeting(meeting);
      setFileName(meeting.source_name ?? null);
      setSegments(meeting.segments);
      setEditingSegmentIndex(null);
      setSpeakerLabels({});
      setStatus({ kind: "idle" });
      setMfu(meeting.mfu ?? null);
    },
    [
      setActiveMeeting,
      setEditingSegmentIndex,
      setFileName,
      setMfu,
      setSegments,
      setSpeakerLabels,
      setStatus,
    ],
  );

  const upsertSummary = useCallback(
    (meeting: Meeting) => {
      setMeetingSummaries((previous) =>
        previous.some((summary) => summary.id === meeting.id)
          ? previous.map((summary) =>
              summary.id === meeting.id ? toSummary(meeting) : summary,
            )
          : [toSummary(meeting), ...previous],
      );
    },
    [setMeetingSummaries],
  );

  const openSelectedMeeting = useCallback(
    async (id: number): Promise<Meeting | null> => {
      const generation = ++meetingOpenGenerationRef.current;
      try {
        const meeting = await openMeeting(id);
        if (generation !== meetingOpenGenerationRef.current) return null;
        applyActiveMeeting(meeting);
        return meeting;
      } catch (error) {
        if (generation === meetingOpenGenerationRef.current) {
          setStatus({ kind: "error", message: String(error) });
        }
        return null;
      }
    },
    [applyActiveMeeting, setStatus],
  );

  const clearActiveMeeting = useCallback(() => {
    activeMeetingIdRef.current = null;
    setActiveMeeting(null);
    setFileName(null);
    setSegments([]);
    setEditingSegmentIndex(null);
    setSpeakerLabels({});
    setMfu(null);
  }, [
    setActiveMeeting,
    setEditingSegmentIndex,
    setFileName,
    setMfu,
    setSegments,
    setSpeakerLabels,
  ]);

  return {
    activeMeetingIdRef,
    meetingOpenGenerationRef,
    applyActiveMeeting,
    upsertSummary,
    openSelectedMeeting,
    clearActiveMeeting,
  };
}
