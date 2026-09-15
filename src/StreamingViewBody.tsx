import type { ComponentProps } from "react";
import { StreamingDialogs } from "./StreamingDialogs";
import { StreamingHeader } from "./StreamingHeader";
import { StreamingInfoBar } from "./StreamingInfoBar";
import { StreamingMfuPanel } from "./StreamingMfuPanel";
import { StreamingSidebar } from "./StreamingSidebar";
import { StreamingTranscriptPanel } from "./StreamingTranscriptPanel";

type HeaderProps = ComponentProps<typeof StreamingHeader>;
type InfoProps = ComponentProps<typeof StreamingInfoBar>;
type SidebarProps = ComponentProps<typeof StreamingSidebar>;
type TranscriptProps = ComponentProps<typeof StreamingTranscriptPanel>;
type DialogProps = ComponentProps<typeof StreamingDialogs>;

export interface StreamingViewModel {
  header: HeaderProps;
  info: InfoProps;
  sidebar: SidebarProps | null;
  transcript: TranscriptProps;
  mfu: ComponentProps<typeof StreamingMfuPanel> | null;
  dialogs: DialogProps;
}

/** Layout composition only. Controller state is passed through narrow child contracts. */
export function StreamingViewBody({ view }: { view: StreamingViewModel }) {
  return (
    <div className="app streaming-view">
      <StreamingHeader {...view.header} />
      <StreamingInfoBar {...view.info} />
      <div className="wp-main">
        {view.sidebar && <StreamingSidebar {...view.sidebar} />}
        <section className="wp-workspace">
          <StreamingTranscriptPanel {...view.transcript} />
          {view.mfu && <StreamingMfuPanel {...view.mfu} />}
        </section>
      </div>
      <StreamingDialogs {...view.dialogs} />
    </div>
  );
}
