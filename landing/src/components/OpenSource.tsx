import { BrandIcon, SERVICE_ICON } from "./brandIcons";
import { CodeIcon, ShieldIcon, StarIcon, UserIcon } from "./icons";
import { CopyBlock, FOCUS_RING, LinkButton, SectionHeading, cn } from "./ui";
import { formatCount, type RepoStats } from "../github";
import { GITHUB_REPO, type LatestRelease } from "../releases";

const PRINCIPLES = [
  { icon: ShieldIcon, label: "No telemetry" },
  { icon: UserIcon, label: "No Zuno account" },
  { icon: CodeIcon, label: "Apache-2.0" },
] as const;

function Stat({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="text-[clamp(1.4rem,3vw,2.5rem)] font-semibold tabular-nums tracking-[-0.04em]">{value}</span>
      <span className="font-mono text-[12px] text-muted-foreground">{label}</span>
    </div>
  );
}

export function OpenSource({ repo, release }: { repo: RepoStats | null; release: LatestRelease | null }) {
  return (
    <section id="open-source">
      <div className="mx-auto max-w-6xl px-6 py-20 lg:py-28">
        <SectionHeading index="02" label="open source" title="Zuno collects nothing." />

        <ul className="mt-8 flex flex-wrap gap-2">
          {PRINCIPLES.map(({ icon: Icon, label }) => (
            <li key={label} className="flex items-center gap-2 rounded-full bg-card/50 py-2 pl-3 pr-4 text-[15px]">
              <Icon size={17} className="text-primary" />
              {label}
            </li>
          ))}
        </ul>

        <div className="mt-12 grid grid-cols-1 gap-4 lg:grid-cols-[1.15fr_0.85fr]">
          <div className="reveal flex flex-col gap-8 rounded-3xl bg-card/40 p-6 sm:p-8">
            <a
              href={GITHUB_REPO}
              rel="noopener"
              className={cn("flex w-fit items-center gap-3 rounded-xl text-lg font-semibold tracking-[-0.01em] hover:text-primary", FOCUS_RING)}
            >
              <BrandIcon icon={SERVICE_ICON.github} width={26} height={26} />
              noFAYZ/zuno
            </a>

            <div className="grid grid-cols-3 gap-4">
              <Stat value={repo ? formatCount(repo.stars) : "—"} label="stars" />
              <Stat value={repo ? formatCount(repo.forks) : "—"} label="forks" />
              <Stat value={release ? `v${release.version}` : "—"} label="latest" />
            </div>

            {repo && repo.contributors.length > 0 ? (
              <ul className="flex -space-x-2" aria-label="Contributors">
                {repo.contributors.slice(0, 10).map((person) => (
                  <li key={person.login}>
                    <a href={person.url} rel="noopener" className={cn("block rounded-full", FOCUS_RING)} title={person.login}>
                      <img
                        src={person.avatar}
                        alt={person.login}
                        width={36}
                        height={36}
                        loading="lazy"
                        className="size-9 rounded-full bg-card ring-2 ring-background"
                      />
                    </a>
                  </li>
                ))}
              </ul>
            ) : null}

            <div className="mt-auto flex flex-wrap gap-3">
              <LinkButton href={GITHUB_REPO} rel="noopener" variant="light">
                <StarIcon size={17} />
                Star on GitHub
              </LinkButton>
              <LinkButton href={`${GITHUB_REPO}/issues`} rel="noopener" variant="ghost">
                Report a bug
              </LinkButton>
            </div>
          </div>

          <div className="reveal flex flex-col gap-5 rounded-3xl bg-card/40 p-6 sm:p-8">
            <div>
              <h3 className="text-lg font-semibold tracking-[-0.01em]">Build it yourself</h3>
              <p className="mt-1 text-[15px] text-muted-foreground">Needs Node.js and Rust.</p>
            </div>
            <CopyBlock
              label="build commands"
              lines={["git clone https://github.com/noFAYZ/zuno", "cd zuno && npm install", "npm run tauri dev"]}
            />
          </div>
        </div>
      </div>
    </section>
  );
}
