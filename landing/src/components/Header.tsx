import { BrandIcon, SERVICE_ICON } from "./brandIcons";
import { PlayingBars } from "./PlayingBars";
import { FOCUS_RING, LinkButton, cn } from "./ui";
import { formatCount } from "../github";
import { GITHUB_REPO } from "../releases";

const NAV = [
  ["Features", "#features"],
  ["Open source", "#open-source"],
] as const;

export function Header({ stars }: { stars: number | null }) {
  return (
    <header className="fixed inset-x-0 top-0 z-40 px-3 pt-3 sm:px-4 sm:pt-4">
      <div className="header-glass mx-auto flex h-14 max-w-6xl items-center gap-2 rounded-full pl-2.5 pr-2">
        <a
          href="#top"
          className={cn("flex items-center gap-2 rounded-full py-1 pl-0.5 pr-2 text-[17px] font-semibold tracking-tight", FOCUS_RING)}
        >
          <img src="./ghost.webp" alt="" width={30} height={30} className="size-[30px]" />
          <span>
            zuno<span className="text-primary">_</span>
          </span>
          <PlayingBars className="now-bars h-3" />
        </a>

        <nav aria-label="Sections" className="mx-auto hidden items-center md:flex">
          {NAV.map(([label, href]) => (
            <a
              key={href}
              href={href}
              className={cn(
                "rounded-full px-3.5 py-2 text-sm text-muted-foreground transition-colors hover:text-foreground",
                FOCUS_RING,
              )}
            >
              {label}
            </a>
          ))}
        </nav>

        <div className="ml-auto flex items-center gap-1 md:ml-0">
          <a
            href={GITHUB_REPO}
            rel="noopener"
            aria-label={stars === null ? "Zuno on GitHub" : `Zuno on GitHub, ${stars} stars`}
            className={cn(
              "hidden h-9 items-center gap-2 rounded-full px-3 text-sm text-muted-foreground transition-colors hover:text-foreground sm:flex",
              FOCUS_RING,
            )}
          >
            <BrandIcon icon={SERVICE_ICON.github} width={17} height={17} />
            {stars === null ? null : <span className="font-mono text-[13px] tabular-nums">{formatCount(stars)}</span>}
          </a>
          <LinkButton href="#download" size="sm">
            Download
          </LinkButton>
        </div>
      </div>
    </header>
  );
}
