// Pure helpers shared by the meeting workspace: language display labels and
// the summary projection used by both the workspace and the library sidebar.

import type { Meeting as PersistedMeeting, MeetingSummary } from "./ipc";

const LANGUAGE_LABELS: Record<string, string> = {
  en: "English",
  ru: "Russian",
  tr: "Turkish",
};

export function formatDetectedLanguage(language: string): string {
  return LANGUAGE_LABELS[language] ?? language;
}

export function toSummary(meeting: PersistedMeeting): MeetingSummary {
  return {
    id: meeting.id,
    title: meeting.title,
    created_at_ms: meeting.created_at_ms,
    duration_ms: meeting.duration_ms,
    status: meeting.status,
  };
}
