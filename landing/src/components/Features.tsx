import type { PointerEvent, ReactNode } from "react";
import { Equalizer } from "./Equalizer";
import { Presence } from "./Presence";
import { ThemeCompare } from "./ThemeCompare";
import { CheckIcon, DownloadIcon, FolderIcon, PauseIcon, PlayIcon, SkipIcon } from "./icons";
import { SectionHeading, cn } from "./ui";
import { TRACK, usePlayer } from "../audio/demoPlayer";

function Cell({ className, title, body, children }: { className?: string; title: string; body: string; children: ReactNode }) {
  return (
    <article className={cn("spotlight reveal flex flex-col gap-6 rounded-3xl bg-card/40 p-5 sm:p-6", className)}>
      <div className="flex flex-col gap-1 px-1">
        <h3 className="text-[17px] font-semibold tracking-[-0.015em]">{title}</h3>
        <p className="text-[15px] text-muted-foreground">{body}</p>
      </div>
      <div className="mt-auto">{children}</div>
    </article>
  );
}

const DOWNLOADS = [
  { name: "Low Tide", state: "done" },
  { name: "Anticipating", state: "active" },
  { name: "Promise", state: "queued" },
] as const;

function OfflineQueue() {
  return (
    <div className="flex flex-col gap-1 rounded-2xl bg-background/80 p-2">
      {DOWNLOADS.map((row) => (
        <div key={row.name} className="flex items-center gap-3 rounded-xl px-2.5 py-2">
          <span
            className={cn(
              "grid size-7 shrink-0 place-items-center rounded-full",
              row.state === "queued" ? "bg-muted text-muted-foreground" : "bg-primary/15 text-primary",
            )}
            aria-hidden="true"
          >
            {row.state === "done" ? <CheckIcon size={14} /> : <DownloadIcon size={14} />}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate text-sm">{row.name}</span>
            <span className="mt-1.5 block h-1 overflow-hidden rounded-full bg-muted">
              <span
                className={cn(
                  "block h-full origin-left rounded-full bg-primary",
                  row.state === "active" && "dl-bar",
                  row.state === "queued" && "scale-x-0",
                )}
              />
            </span>
          </span>
        </div>
      ))}
      <div className="flex items-center gap-3 rounded-xl px-2.5 py-2">
        <span className="grid size-7 shrink-0 place-items-center rounded-full bg-muted text-muted-foreground" aria-hidden="true">
          <FolderIcon size={14} />
        </span>
        <span className="min-w-0 truncate font-mono text-[12px] text-muted-foreground">~/Music/FLAC</span>
      </div>
    </div>
  );
}

/** The app's capsule; hover the card to open it. */
function MiniPreview() {
  const { playing } = usePlayer();
  return (
    <div className="grid min-h-[13.5rem] place-items-center rounded-2xl bg-background/80 p-6">
      <div className="flex h-11 w-40 items-center gap-2 overflow-hidden rounded-[22px] bg-popover pl-1.5 pr-3 shadow-xl transition-[width,height,border-radius] duration-300 [transition-timing-function:var(--ease-out-expo)] group-hover/cell:h-24 group-hover/cell:w-64 group-hover/cell:flex-wrap group-hover/cell:rounded-3xl group-hover/cell:py-2">
        <img src={TRACK.cover} alt="" className="size-8 shrink-0 rounded-full bg-card p-0.5" />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[12px] font-semibold">{TRACK.title}</span>
          <span className="block truncate text-[10px] text-muted-foreground">{TRACK.artist}</span>
        </span>
        <span className="hidden w-full items-center justify-center gap-2 group-hover/cell:flex" aria-hidden="true">
          <SkipIcon back size={14} className="text-muted-foreground" />
          <span className="grid size-7 place-items-center rounded-full bg-foreground text-background">
            {playing ? <PauseIcon size={12} /> : <PlayIcon size={12} />}
          </span>
          <SkipIcon size={14} className="text-muted-foreground" />
        </span>
      </div>
    </div>
  );
}

export function Features() {
  // One listener for the grid: it moves the light inside whichever card the pointer is on.
  const moveSpotlight = (event: PointerEvent<HTMLDivElement>) => {
    const cell = (event.target as Element).closest<HTMLElement>(".spotlight");
    if (!cell) return;
    const box = cell.getBoundingClientRect();
    cell.style.setProperty("--mx", `${event.clientX - box.left}px`);
    cell.style.setProperty("--my", `${event.clientY - box.top}px`);
  };

  return (
    <section id="features">
      <div className="mx-auto max-w-6xl px-6 py-20 lg:py-28">
        <SectionHeading index="01" label="the details" title="Everything a browser tab can't do." />

        <div className="mt-14 grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-6" onPointerMove={moveSpotlight}>
          <Cell className="lg:col-span-3" title="Ten-band EQ" body="The app's own curve. Press play, then drag.">
            <Equalizer />
          </Cell>
          <Cell className="lg:col-span-3" title="Light or dark" body="Drag to compare.">
            <ThemeCompare />
          </Cell>
          <Cell className="lg:col-span-2" title="Discord & Last.fm" body="Both follow the song.">
            <Presence />
          </Cell>
          <Cell className="lg:col-span-2" title="Offline & local files" body="Playlists, albums, your own folders.">
            <OfflineQueue />
          </Cell>
          <Cell className="group/cell md:col-span-2 lg:col-span-2" title="Mini player" body="Floats on top when you look away.">
            <MiniPreview />
          </Cell>
        </div>
      </div>
    </section>
  );
}
