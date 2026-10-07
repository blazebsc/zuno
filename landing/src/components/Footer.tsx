import { useEffect, useRef } from "react";
import { ArrowDownIcon } from "./icons";
import { FOCUS_RING, LinkButton, cn } from "./ui";
import { GITHUB_REPO, RELEASES_URL } from "../releases";
import { useReducedMotion } from "../useReducedMotion";

const LINKS = [
  ["GitHub", GITHUB_REPO],
  ["Releases", RELEASES_URL],
  ["Issues", `${GITHUB_REPO}/issues`],
  ["Licence", `${GITHUB_REPO}/blob/main/LICENSE`],
] as const;

export function Footer() {
  const reduced = useReducedMotion();
  const footerRef = useRef<HTMLElement>(null);

  // The floating mini player steps aside here, so it never sits on the last lines of the page.
  useEffect(() => {
    const footer = footerRef.current;
    if (!footer) return;
    const observer = new IntersectionObserver(([entry]) => {
      document.documentElement.dataset.footerIn = String(entry.isIntersecting);
    });
    observer.observe(footer);
    return () => observer.disconnect();
  }, []);

  return (
    <footer ref={footerRef} className="grain relative isolate overflow-hidden">
      <div className="mx-auto flex max-w-6xl flex-col gap-12 px-6 pt-24 sm:flex-row sm:items-end sm:justify-between">
        <div className="max-w-md">
          <p className="text-balance text-[clamp(1.9rem,3.4vw,2.75rem)] font-semibold leading-[1.05] tracking-[-0.04em]">
            Close the tab. Keep the music<span className="text-primary">_</span>
          </p>
          <div className="mt-7 flex flex-wrap gap-3">
            <LinkButton href="#download">Download Zuno</LinkButton>
            <LinkButton href={GITHUB_REPO} rel="noopener" variant="ghost">
              View the source
            </LinkButton>
          </div>
        </div>

        <nav aria-label="Links" className="flex flex-wrap gap-x-6 gap-y-3">
          {LINKS.map(([label, href]) => (
            <a
              key={label}
              href={href}
              rel="noopener"
              className={cn("rounded-md text-[15px] text-muted-foreground transition-colors hover:text-foreground", FOCUS_RING)}
            >
              {label}
            </a>
          ))}
        </nav>
      </div>

      <div className="relative mt-16 select-none overflow-hidden" aria-hidden="true">
        <p className="wordmark-xl text-center font-semibold">zuno</p>
        <div className="screen-ghost absolute bottom-0 right-[6%] w-[17%] max-w-52">
          <video className="float aspect-square w-full object-cover" src="./zuno-character.mp4" autoPlay={!reduced} muted loop playsInline />
        </div>
      </div>

      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-6 pb-10 pt-6 font-mono text-[12px] text-muted-foreground sm:flex-row sm:items-center sm:justify-between">
        <p className="max-w-xl text-pretty">
          Unofficial. Not affiliated with YouTube or Google.
        </p>
        <a href="#top" className={cn("flex shrink-0 items-center gap-1.5 rounded-md hover:text-foreground", FOCUS_RING)}>
          Back to top
          <ArrowDownIcon size={14} className="rotate-180" />
        </a>
      </div>
    </footer>
  );
}
