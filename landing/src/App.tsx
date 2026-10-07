import { Downloads } from "./components/Downloads";
import { Features } from "./components/Features";
import { Film } from "./components/Film";
import { Footer } from "./components/Footer";
import { Header } from "./components/Header";
import { Hero } from "./components/Hero";
import { MiniPlayer } from "./components/MiniPlayer";
import { OpenSource } from "./components/OpenSource";
import { FOCUS_RING, cn } from "./components/ui";
import { useRepoStats } from "./github";
import { useLatestRelease } from "./useLatestRelease";

export function App() {
  const { release, platform } = useLatestRelease();
  const repo = useRepoStats();
  const stars = repo?.stars ?? null;

  return (
    <>
      <a
        href="#download"
        className={cn(
          "sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-[60] focus:rounded-full focus:bg-foreground focus:px-4 focus:py-2 focus:text-background",
          FOCUS_RING,
        )}
      >
        Skip to download
      </a>
      <Header stars={stars} />
      <main>
        <Hero release={release} platform={platform} stars={stars} />
        <Film />
        <Features />
        <OpenSource repo={repo} release={release} />
        <Downloads release={release} platform={platform} />
      </main>
      <Footer />
      <MiniPlayer />
    </>
  );
}
