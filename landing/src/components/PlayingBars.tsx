import { cn } from "./ui";

/** The app's playing indicator; the bars only move while the demo track is playing. */
export function PlayingBars({ className }: { className?: string }) {
  return (
    <span className={cn("flex items-end gap-[2px]", className)} aria-hidden="true">
      {[0, 1, 2].map((bar) => (
        <span
          key={bar}
          className="equaliser-bar w-[2.5px] rounded-full bg-primary"
          style={{ animationDelay: `${bar * -0.37}s` }}
        />
      ))}
    </span>
  );
}
