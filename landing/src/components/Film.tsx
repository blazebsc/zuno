import { useReducedMotion } from "../useReducedMotion";

const DEMO_VIDEO = "https://pub-493a5d4ea10b45dcaa83917aa3856a32.r2.dev/zunodem.mp4";

/** The app itself, recorded. It pins and grows to full bleed while the page scrolls past (styles.css). */
export function Film() {
  const reduced = useReducedMotion();

  return (
    <section aria-label="Zuno, recorded" className="film-stage">
      <div className="film-sticky">
        <div className="film-glow" aria-hidden="true" />
        <div className="film-frame">
          <video
            className="absolute inset-0 size-full object-cover"
            width={1234}
            height={922}
            src={DEMO_VIDEO}
            poster="./theme-dark.webp"
            autoPlay={!reduced}
            muted
            loop
            playsInline
            preload="metadata"
            aria-hidden="true"
          />
        </div>
      </div>
    </section>
  );
}
