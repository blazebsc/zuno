import { useState } from "react";
import { ArrowsHorizontalIcon } from "./icons";

/** Before/after of the same screen in both themes; a native range input does the dragging and the keys. */
export function ThemeCompare() {
  const [split, setSplit] = useState(52);

  return (
    <div className="group relative aspect-[1405/1014] overflow-hidden rounded-2xl bg-background">
      <img
        src="./theme-dark.webp"
        alt="Zuno's home screen in the dark theme"
        width={1405}
        height={1014}
        loading="lazy"
        decoding="async"
        className="absolute inset-0 size-full object-cover"
      />
      <img
        src="./theme-light.webp"
        alt="The same screen in the light theme"
        width={1405}
        height={1014}
        loading="lazy"
        decoding="async"
        className="absolute inset-0 size-full object-cover"
        style={{ clipPath: `inset(0 0 0 ${split}%)` }}
      />
      <div className="pointer-events-none absolute inset-y-0 w-0.5 -translate-x-1/2 bg-foreground" style={{ left: `${split}%` }} aria-hidden="true">
        <span className="absolute left-1/2 top-1/2 grid size-9 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full bg-foreground text-background shadow-lg group-has-[input:focus-visible]:ring-2 group-has-[input:focus-visible]:ring-ring">
          <ArrowsHorizontalIcon size={16} />
        </span>
      </div>
      <span className="pointer-events-none absolute left-3 top-3 rounded-full bg-background/75 px-2.5 py-1 font-mono text-[11px] backdrop-blur">dark</span>
      <span className="pointer-events-none absolute right-3 top-3 rounded-full bg-background/75 px-2.5 py-1 font-mono text-[11px] backdrop-blur">light</span>
      <input
        type="range"
        min={0}
        max={100}
        value={split}
        onChange={(event) => setSplit(Number(event.currentTarget.value))}
        aria-label="Compare the dark and light themes"
        className="absolute inset-0 size-full cursor-ew-resize opacity-0"
      />
    </div>
  );
}
