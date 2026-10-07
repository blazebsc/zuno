import { useState, type AnchorHTMLAttributes, type ReactNode } from "react";
import { CheckIcon, CopyIcon } from "./icons";

/** A plain join: nothing here overrides classes, so tailwind-merge would not earn its bytes. */
export function cn(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(" ");
}

export const FOCUS_RING = "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

const BASE =
  "group inline-flex shrink-0 select-none items-center justify-center gap-2 rounded-full font-semibold tracking-[-0.01em] " +
  "transition-[background-color,color,transform] duration-150 ease-out active:translate-y-px " +
  FOCUS_RING;

const SIZES = {
  sm: "h-9 px-4 text-sm",
  md: "h-11 px-5 text-[15px]",
  lg: "h-13 px-7 text-[15px]",
} as const;

const VARIANTS = {
  solid: "bg-primary text-primary-foreground hover:bg-primary/88",
  light: "bg-foreground text-background hover:bg-foreground/88",
  ghost: "bg-card/50 text-foreground hover:bg-card",
} as const;

export function LinkButton({
  variant = "solid",
  size = "md",
  className,
  ...props
}: AnchorHTMLAttributes<HTMLAnchorElement> & { variant?: keyof typeof VARIANTS; size?: keyof typeof SIZES }) {
  return <a className={cn(BASE, SIZES[size], VARIANTS[variant], className)} {...props} />;
}

export function SectionHeading({
  index,
  label,
  title,
  lede,
}: {
  index: string;
  label: string;
  title: ReactNode;
  lede?: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-5">
      <p className="flex items-center gap-3 font-mono text-[12px] uppercase tracking-[0.16em] text-muted-foreground">
        <span className="text-primary">{index}</span>
        <span className="h-px w-8 bg-muted-foreground/40" aria-hidden="true" />
        {label}
      </p>
      <h2 className="max-w-[20ch] text-balance text-[clamp(2.25rem,4.6vw,4rem)] font-semibold leading-[1.02] tracking-[-0.042em]">
        {title}
      </h2>
      {lede ? <p className="max-w-[46ch] text-pretty text-lg leading-relaxed text-muted-foreground">{lede}</p> : null}
    </div>
  );
}

/** Shell commands with one copy button; still selectable by hand where the clipboard is blocked. */
export function CopyBlock({ lines, label }: { lines: readonly string[]; label: string }) {
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(lines.join("\n"));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // Blocked clipboard: the text is still there to select.
    }
  };

  return (
    <div className="flex items-start gap-3 rounded-2xl bg-background/70 py-3 pl-4 pr-2">
      <pre className="min-w-0 flex-1 overflow-x-auto py-1 font-mono text-[13px] leading-6">
        {lines.map((line) => (
          <span key={line} className="block whitespace-pre">
            <span className="select-none text-primary">$ </span>
            {line}
          </span>
        ))}
      </pre>
      <button
        type="button"
        onClick={copy}
        className={cn(
          "grid size-9 shrink-0 place-items-center rounded-xl text-muted-foreground transition-colors hover:bg-card hover:text-foreground",
          FOCUS_RING,
        )}
        aria-label={`Copy ${label}`}
      >
        {copied ? <CheckIcon size={16} /> : <CopyIcon size={16} />}
      </button>
      <span className="sr-only" role="status" aria-live="polite">
        {copied ? "Copied to clipboard" : ""}
      </span>
    </div>
  );
}
